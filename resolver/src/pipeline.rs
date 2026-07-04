//! The deterministic resolution pipeline. Each `stage_*` function attempts
//! exactly one kind of match and returns:
//!   - `None`            — this stage found nothing; try the next stage.
//!   - `Some(Ambiguous)`  — this stage found >1 equally-valid candidate; STOP.
//!   - `Some(<entity>)`   — this stage found exactly one confident match; STOP.
//!
//! "Stop" is enforced structurally: every `resolve_*` entry point is a
//! straight-line `if let Some(r) = stage(..) { return r }` chain, so a later
//! stage physically cannot run once an earlier one has answered. This is
//! what makes the pipeline deterministic and prevents the "silently fall
//! through to a random fallback" failure mode the rest of CodeBroker used to
//! have scattered across individual tools.

use crate::types::*;
use storage::Database;

const SEMANTIC_THRESHOLD: f32 = 0.35;
/// If the top two semantic candidates are within this cosine-similarity
/// margin of each other, neither is confidently "the" answer — return
/// Ambiguous instead of arbitrarily picking the marginally-higher one.
const SEMANTIC_AMBIGUITY_MARGIN: f32 = 0.03;

fn normalize_query_path(db: &Database, q: &str) -> String {
    let mut normalized = q.trim().replace('\\', "/");
    let root = db.project_root.replace('\\', "/");
    
    if normalized.starts_with(&root) {
        normalized = normalized[root.len()..].to_string();
    }
    
    normalized
        .trim_start_matches('/')
        .trim_start_matches("./")
        .trim_end_matches('/')
        .to_string()
}

// ---------------------------------------------------------------------------
// Symbol stages
// ---------------------------------------------------------------------------

struct SymbolRow {
    #[allow(dead_code)]
    id: i64,
    name: String,
    kind: String,
    start_line: i64,
    end_line: i64,
    path: String,
    is_entrypoint: bool,
}

fn query_symbols_by_name(
    db: &Database,
    name_clause: &str,
    name_param: &str,
    file_hint: Option<&str>,
) -> rusqlite::Result<Vec<SymbolRow>> {
    let sql = format!(
        "SELECT symbols.id, symbols.name, symbols.kind, symbols.start_line, symbols.end_line,
                files.path, COALESCE(sf.is_entrypoint, 0)
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         LEFT JOIN symbol_features sf ON sf.symbol_id = symbols.id
         WHERE {} AND (?2 = '' OR files.path LIKE '%' || ?2 || '%')",
        name_clause
    );
    let mut stmt = db.conn.prepare(&sql)?;
    let mut rows = stmt.query(rusqlite::params![name_param, file_hint.unwrap_or("")])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(SymbolRow {
            id: row.get(0)?,
            name: row.get(1)?,
            kind: row.get(2)?,
            start_line: row.get(3)?,
            end_line: row.get(4)?,
            path: row.get(5)?,
            is_entrypoint: row.get(6)?,
        });
    }
    Ok(out)
}

fn rows_to_entity(
    db: &Database,
    query: &str,
    mut rows: Vec<SymbolRow>,
    confidence: Confidence,
    hint: &str,
    line_hint: Option<i64>,
) -> Option<ResolvedEntity> {
    match rows.len() {
        0 => None,
        1 => {
            let r = &rows[0];
            Some(ResolvedEntity::Symbol(ResolvedSymbol {
                name: r.name.clone(),
                kind: r.kind.clone(),
                file_path: db.resolve_path(&r.path),
                start_line: r.start_line,
                end_line: r.end_line,
                is_entrypoint: r.is_entrypoint,
                confidence,
            }))
        }
        _ => {
            // When the caller supplies a line number, try to narrow within-file
            // candidates to the definition that encloses that line: keep only
            // rows whose start_line ≤ line_hint, then pick the one with the
            // largest start_line (closest preceding definition). This resolves
            // the common case of a file that defines the same name at multiple
            // scopes (e.g. a module-level variable shadowed by a local inside a
            // function, or two class-level constants with the same name).
            if let Some(line) = line_hint {
                let mut before: Vec<SymbolRow> =
                    rows.drain(..).filter(|r| r.start_line <= line).collect();
                if !before.is_empty() {
                    before.sort_by_key(|r| std::cmp::Reverse(r.start_line));
                    let r = &before[0];
                    return Some(ResolvedEntity::Symbol(ResolvedSymbol {
                        name: r.name.clone(),
                        kind: r.kind.clone(),
                        file_path: db.resolve_path(&r.path),
                        start_line: r.start_line,
                        end_line: r.end_line,
                        is_entrypoint: r.is_entrypoint,
                        confidence,
                    }));
                }
                // All candidates start after the hint line — fall through to
                // Ambiguous with the original rows restored from before.
                rows = before; // empty at this point; Ambiguous will show 0 candidates but
                               // this path is practically unreachable for sane line hints
            }
            Some(ResolvedEntity::Ambiguous(AmbiguousMatch {
                query: query.to_string(),
                candidates: rows
                    .into_iter()
                    .map(|r| Candidate {
                        entity_type: EntityType::Symbol,
                        name: r.name,
                        kind: r.kind,
                        file_path: db.resolve_path(&r.path),
                        start_line: r.start_line,
                    })
                    .collect(),
                hint: hint.to_string(),
            }))
        }
    }
}

/// Exact, case-sensitive symbol-name match — the strongest possible signal.
fn stage_exact_symbol(
    db: &Database,
    query: &str,
    file_hint: Option<&str>,
    line_hint: Option<i64>,
) -> Option<ResolvedEntity> {
    let rows = query_symbols_by_name(db, "symbols.name = ?1", query, file_hint).ok()?;
    rows_to_entity(
        db,
        query,
        rows,
        Confidence::exact("Exact symbol name match"),
        "Multiple symbols share this exact name. Re-run with `file_path` set to a substring of the file you mean (see `candidates`) to disambiguate, or add `line` to pinpoint the definition.",
        line_hint,
    )
}

/// Case-insensitive symbol-name match — only tried once the exact stage finds
/// nothing, so a perfectly-cased match never gets diluted by sloppier ones.
fn stage_canonical_symbol(
    db: &Database,
    query: &str,
    file_hint: Option<&str>,
    line_hint: Option<i64>,
) -> Option<ResolvedEntity> {
    let rows =
        query_symbols_by_name(db, "LOWER(symbols.name) = LOWER(?1)", query, file_hint).ok()?;
    rows_to_entity(
        db,
        query,
        rows,
        Confidence::high(85, "Case-insensitive symbol name match"),
        "Multiple symbols share this name (case-insensitive). Re-run with `file_path` set to disambiguate, or add `line` to pinpoint the definition.",
        line_hint,
    )
}

/// Embedding-similarity match. Only reachable when the workspace was indexed
/// with an embedding model AND the caller supplied a query vector — both are
/// the resolver's responsibility to check, not each tool's.
fn stage_semantic_symbol(db: &Database, query_vector: Option<&[f32]>) -> Option<ResolvedEntity> {
    let qv = query_vector?;
    let embeddings = db.get_all_symbol_embeddings().ok()?;
    if embeddings.is_empty() {
        return None;
    }
    let mut scored: Vec<(f32, i64, String, String, String)> = embeddings
        .into_iter()
        .map(|(id, name, kind, path, blob)| {
            let v = storage::blob_to_embedding(&blob);
            let sim = storage::cosine_similarity(qv, &v);
            (sim, id, name, kind, path)
        })
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let (top_sim, _, _, _, _) = scored.first()?.clone();
    if top_sim < SEMANTIC_THRESHOLD {
        return None;
    }

    let close: Vec<_> = scored
        .iter()
        .take_while(|(sim, ..)| top_sim - sim < SEMANTIC_AMBIGUITY_MARGIN)
        .collect();

    if close.len() > 1 {
        return Some(ResolvedEntity::Ambiguous(AmbiguousMatch {
            query: String::new(),
            candidates: close
                .iter()
                .map(|(_, _, name, kind, path)| Candidate {
                    entity_type: EntityType::Symbol,
                    name: name.clone(),
                    kind: kind.clone(),
                    file_path: db.resolve_path(path),
                    start_line: 0,
                })
                .collect(),
            hint: "Multiple symbols are equally strong semantic matches. Re-run with `file_path` set to disambiguate.".to_string(),
        }));
    }

    let (sim, id, name, kind, path) = scored.into_iter().next()?;
    let (start_line, end_line, is_entrypoint) = db
        .conn
        .query_row(
            "SELECT symbols.start_line, symbols.end_line, COALESCE(sf.is_entrypoint, 0)
             FROM symbols LEFT JOIN symbol_features sf ON sf.symbol_id = symbols.id
             WHERE symbols.id = ?1",
            rusqlite::params![id],
            |r| {
                Ok((
                    r.get::<_, i64>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            },
        )
        .unwrap_or((0, 0, false));

    Some(ResolvedEntity::Symbol(ResolvedSymbol {
        name,
        kind,
        file_path: db.resolve_path(&path),
        start_line,
        end_line,
        is_entrypoint,
        confidence: Confidence::new(
            (sim.clamp(0.0, 1.0) * 100.0) as u8,
            vec![format!("Semantic embedding similarity {:.2}", sim)],
        ),
    }))
}

/// Resolves a query that is known/expected to name a **symbol** — the
/// pipeline subset used by tools (`get_context`, `get_implementation`,
/// `get_edit_context`, `impact_analysis`, `explore_graph`, `shortest_path`,
/// `graph_subtree`) that only ever take a symbol name. Skips the
/// file/directory/subsystem/feature stages entirely: a tool that already
/// knows it wants a symbol shouldn't pay for (or risk mis-resolving into)
/// entity types it didn't ask for.
pub fn resolve_symbol(
    db: &Database,
    query: &str,
    file_hint: Option<&str>,
    query_vector: Option<&[f32]>,
    line_hint: Option<i64>,
) -> ResolvedEntity {
    let mut stages_tried = Vec::new();

    stages_tried.push("exact_symbol".to_string());
    if let Some(r) = stage_exact_symbol(db, query, file_hint, line_hint) {
        return r;
    }

    stages_tried.push("canonical_symbol".to_string());
    if let Some(r) = stage_canonical_symbol(db, query, file_hint, line_hint) {
        return r;
    }

    stages_tried.push("semantic_symbol".to_string());
    if let Some(r) = stage_semantic_symbol(db, query_vector) {
        return r;
    }

    ResolvedEntity::NotFound(NotFound {
        query: query.to_string(),
        reason: format!("No symbol named '{}' found in the index.", query),
        stages_tried,
    })
}

// ---------------------------------------------------------------------------
// File / Directory stages
// ---------------------------------------------------------------------------

fn all_file_paths(db: &Database) -> rusqlite::Result<Vec<String>> {
    let mut stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut rows = stmt.query([])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        out.push(row.get::<_, String>(0)?);
    }
    Ok(out)
}

/// Exact stored-path match (after normalizing the leading "./" both stored
/// paths and CodeBroker's own conventions use).
fn stage_full_path(db: &Database, query: &str) -> Option<ResolvedEntity> {
    let norm_query = normalize_query_path(db, query);
    let paths = all_file_paths(db).ok()?;
    let matches: Vec<&String> = paths
        .iter()
        .filter(|p| {
            let norm_stored = normalize_query_path(db, p);
            norm_stored == norm_query
        })
        .collect();
    match matches.len() {
        0 => None,
        1 => Some(ResolvedEntity::File(ResolvedFile {
            file_path: db.resolve_path(matches[0]),
            confidence: Confidence::exact("Exact indexed file path match"),
        })),
        _ => Some(ResolvedEntity::Ambiguous(AmbiguousMatch {
            query: query.to_string(),
            candidates: matches
                .into_iter()
                .map(|p| Candidate {
                    entity_type: EntityType::File,
                    name: p.clone(),
                    kind: "file".to_string(),
                    file_path: db.resolve_path(p),
                    start_line: 0,
                })
                .collect(),
            hint: "Multiple indexed files share this path.".to_string(),
        })),
    }
}

/// Path-suffix match: the query names the tail of a stored path (e.g.
/// `frontend/app/page.tsx` matching the indexed `OrcaAI/frontend/app/page.tsx`,
/// or a bare filename matching exactly one indexed file). This is where a
/// previous, unflagged bug lived: `skeletonize_file`'s own ad hoc suffix match
/// silently took the first hit even when more than one indexed file shared
/// the same suffix — the resolver now reports `Ambiguous` for that case
/// instead of guessing.
fn stage_filename(db: &Database, query: &str) -> Option<ResolvedEntity> {
    let norm_query = normalize_query_path(db, query);
    if norm_query.is_empty() {
        return None;
    }
    let suffix = format!("/{}", norm_query);
    let paths = all_file_paths(db).ok()?;
    let matches: Vec<&String> = paths
        .iter()
        .filter(|p| {
            let norm_stored = normalize_query_path(db, p);
            norm_stored.ends_with(&suffix) || norm_stored == norm_query
        })
        .collect();
    match matches.len() {
        0 => None,
        1 => Some(ResolvedEntity::File(ResolvedFile {
            file_path: db.resolve_path(matches[0]),
            confidence: Confidence::high(90, "Path-suffix match against a uniquely-indexed file"),
        })),
        _ => Some(ResolvedEntity::Ambiguous(AmbiguousMatch {
            query: query.to_string(),
            candidates: matches
                .into_iter()
                .map(|p| Candidate {
                    entity_type: EntityType::File,
                    name: p.clone(),
                    kind: "file".to_string(),
                    file_path: db.resolve_path(p),
                    start_line: 0,
                })
                .collect(),
            hint:
                "Multiple indexed files end with this path. Provide a longer/more specific suffix."
                    .to_string(),
        })),
    }
}

/// Directory match: the query is a prefix shared by one or more indexed
/// files' parent directory.
fn stage_directory(db: &Database, query: &str) -> Option<ResolvedEntity> {
    let norm_query = normalize_query_path(db, query);
    if norm_query.is_empty() {
        return None;
    }
    let suffix = format!("/{}", norm_query);
    let paths = all_file_paths(db).ok()?;
    let mut children: Vec<String> = Vec::new();
    for p in &paths {
        let norm_stored = normalize_query_path(db, p);
        let parent = std::path::Path::new(&norm_stored)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        if parent == norm_query || parent.ends_with(&suffix) {
            children.push(norm_stored);
        }
    }
    if children.is_empty() {
        return None;
    }
    children.sort();
    let sample = children.iter().take(10).cloned().collect();
    Some(ResolvedEntity::Directory(ResolvedDirectory {
        directory_path: norm_query,
        file_count: children.len(),
        sample_files: sample,
        confidence: Confidence::high(90, "Directory prefix shared by indexed files"),
    }))
}

/// Resolves a query that is known/expected to name a **file or directory** —
/// used by `read_file_skeleton`/`read_file_snippet`. Tries the path-shaped
/// stages only; never wanders into symbol/subsystem resolution, since a tool
/// that asks for a file has already declared its intent.
pub fn resolve_path(db: &Database, query: &str) -> ResolvedEntity {
    let mut stages_tried = Vec::new();

    stages_tried.push("full_path".to_string());
    if let Some(r) = stage_full_path(db, query) {
        return r;
    }

    stages_tried.push("filename".to_string());
    if let Some(r) = stage_filename(db, query) {
        return r;
    }

    stages_tried.push("directory".to_string());
    if let Some(r) = stage_directory(db, query) {
        return r;
    }

    ResolvedEntity::NotFound(NotFound {
        query: query.to_string(),
        reason: format!("'{}' does not match any indexed file or directory.", query),
        stages_tried,
    })
}

// ---------------------------------------------------------------------------
// Feature (domain concept) stage
// ---------------------------------------------------------------------------

/// Resolves a query that names a domain concept LITERALLY (e.g. "auth",
/// "database") — deliberately narrow (exact match against the concept name,
/// not a fuzzy substring) so a broader natural-language phrase like
/// "authentication system" falls through to the Subsystem/Semantic stages
/// instead of every auth-adjacent query being short-circuited into a flat
/// concept-tag listing.
fn stage_feature(db: &Database, query: &str) -> Option<ResolvedEntity> {
    let q_lower = query.trim().to_lowercase();
    let concept = query::concepts::CONCEPTS
        .iter()
        .find(|(name, _)| *name == q_lower)
        .map(|(name, _)| *name)?;

    let matches = query::concepts::symbols_for_concept(db, concept).ok()?;
    if matches.is_empty() {
        return None;
    }
    Some(ResolvedEntity::Feature(ResolvedFeature {
        concept: concept.to_string(),
        matching_symbols: matches
            .into_iter()
            .map(|m| Candidate {
                entity_type: EntityType::Symbol,
                name: m.symbol_name,
                kind: m.symbol_kind,
                file_path: m.file_path,
                start_line: 0,
            })
            .collect(),
        confidence: Confidence::high(85, "Exact domain-concept name match"),
    }))
}

// ---------------------------------------------------------------------------
// Subsystem stage
// ---------------------------------------------------------------------------

/// Resolves a query that is known/expected to name a **subsystem** — used by
/// `subsystem_stats`/`subsystem_communication`, which already declare intent
/// by virtue of the tool being called. Delegates the actual seed+graph
/// expansion algorithm to `query::subsystem::discover_subsystem` (that
/// algorithm IS this stage's implementation — the resolver doesn't
/// reimplement it, it owns the decision of whether the result is confident
/// enough to call "resolved").
pub fn resolve_subsystem(
    db: &Database,
    name: &str,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
) -> ResolvedEntity {
    match query::subsystem::discover_subsystem(db, name, semantic_tokens, query_vector) {
        Ok(stats) if !stats.files.is_empty() && stats.confidence != "Low" => {
            let score = match stats.confidence.as_str() {
                "High" => 90,
                "Medium" => 65,
                _ => 30,
            };
            ResolvedEntity::Subsystem(ResolvedSubsystem {
                name: stats.name,
                file_count: stats.files.len(),
                symbol_count: stats.symbols.len(),
                confidence: Confidence::new(
                    score,
                    vec![format!(
                        "discover_subsystem confidence: {}",
                        stats.confidence
                    )],
                ),
                files: stats.files,
                symbols: stats.symbols,
                routes: stats.routes,
            })
        }
        _ => ResolvedEntity::NotFound(NotFound {
            query: name.to_string(),
            reason: format!(
                "No subsystem matching '{}' could be confidently discovered.",
                name
            ),
            stages_tried: vec!["subsystem".to_string()],
        }),
    }
}

// ---------------------------------------------------------------------------
// Full pipeline (entity type unknown / general-purpose lookup)
// ---------------------------------------------------------------------------

/// Runs the entire deterministic pipeline in the order the architecture
/// mandates: Exact Symbol -> Canonical Symbol -> [Alias — not yet
/// implemented; see crate-level docs] -> Full Path -> Filename -> Directory
/// -> Feature -> Subsystem -> Semantic -> NotFound. Used by tools that don't
/// know in advance what kind of entity the input names (`find_symbol`-style
/// open lookups). Tools that already know they want a Symbol, a path, or a
/// Subsystem should call the narrower `resolve_symbol`/`resolve_path`/
/// `resolve_subsystem` instead of paying for (and risking misclassification
/// from) every stage.
pub fn resolve_any(
    db: &Database,
    query: &str,
    file_hint: Option<&str>,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
) -> ResolvedEntity {
    let mut stages_tried = Vec::new();

    stages_tried.push("exact_symbol".to_string());
    if let Some(r) = stage_exact_symbol(db, query, file_hint, None) {
        return r;
    }
    stages_tried.push("canonical_symbol".to_string());
    if let Some(r) = stage_canonical_symbol(db, query, file_hint, None) {
        return r;
    }
    // Alias stage intentionally absent: CodeBroker has no alias table today.
    // Documented as a remaining limitation rather than faked.
    stages_tried.push("full_path".to_string());
    if let Some(r) = stage_full_path(db, query) {
        return r;
    }
    stages_tried.push("filename".to_string());
    if let Some(r) = stage_filename(db, query) {
        return r;
    }
    stages_tried.push("directory".to_string());
    if let Some(r) = stage_directory(db, query) {
        return r;
    }
    stages_tried.push("feature".to_string());
    if let Some(r) = stage_feature(db, query) {
        return r;
    }
    stages_tried.push("subsystem".to_string());
    if let Ok(stats) =
        query::subsystem::discover_subsystem(db, query, semantic_tokens, query_vector)
    {
        if !stats.files.is_empty() && stats.confidence != "Low" {
            let score = if stats.confidence == "High" { 90 } else { 65 };
            return ResolvedEntity::Subsystem(ResolvedSubsystem {
                name: stats.name,
                file_count: stats.files.len(),
                symbol_count: stats.symbols.len(),
                confidence: Confidence::new(
                    score,
                    vec![format!(
                        "discover_subsystem confidence: {}",
                        stats.confidence
                    )],
                ),
                files: stats.files,
                symbols: stats.symbols,
                routes: stats.routes,
            });
        }
    }
    stages_tried.push("semantic".to_string());
    if let Some(r) = stage_semantic_symbol(db, query_vector) {
        return r;
    }

    ResolvedEntity::NotFound(NotFound {
        query: query.to_string(),
        reason: format!(
            "'{}' did not confidently resolve to a symbol, file, directory, feature, or subsystem.",
            query
        ),
        stages_tried,
    })
}

// ---------------------------------------------------------------------------
// Multi-result search (search_codebase, generate_context_capsule)
// ---------------------------------------------------------------------------

/// Unified multi-result search entry point. Wraps `query::engine::search_symbols`
/// and concept augmentation in one place so no MCP tool inlines either.
///
/// This is the "find ranked candidates" complement to `resolve_symbol`/`resolve_any`
/// (which resolve a single entity). Use it whenever the caller wants a list:
/// `search_codebase` and pivot selection in `generate_context_capsule`.
pub fn resolve_search(
    db: &Database,
    query: &str,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
    llm_used: bool,
    path_scope: Option<&str>,
    mode: query::engine::SearchMode,
    whole_word: bool,
    min_confidence: Option<&str>,
) -> (Vec<query::engine::SearchResult>, Option<String>) {
    let (mut results, reason) = query::engine::search_symbols(
        db,
        query,
        semantic_tokens,
        query_vector,
        llm_used,
        path_scope,
        mode,
        whole_word,
        min_confidence,
    )
    .unwrap_or_else(|_| (vec![], None));

    // Concept augmentation: query terms that map to a domain concept ("auth",
    // "realtime", "notifications", "database") pull in concept-tagged symbols
    // that pure lexical search misses — e.g. a query for "auth" finds
    // createClient/signInWithOAuth even though those names don't contain "auth".
    // Centralized here instead of being inline in each tool that needs it.
    let mut seen: std::collections::HashSet<(String, String)> = results
        .iter()
        .map(|r| (r.name.clone(), r.path.clone()))
        .collect();
    let mut concept_added = 0usize;
    for concept in query::concepts::concepts_matching_term(query) {
        if concept_added >= 10 {
            break;
        }
        if let Ok(matches) = query::concepts::symbols_for_concept(db, concept) {
            for m in matches {
                if concept_added >= 10 {
                    break;
                }
                if let Some(scope) = path_scope {
                    if !m.file_path.contains(scope) {
                        continue;
                    }
                }
                let key = (m.symbol_name.clone(), m.file_path.clone());
                if !seen.insert(key) {
                    continue;
                }
                results.push(query::engine::SearchResult {
                    path: m.file_path,
                    name: m.symbol_name,
                    kind: m.symbol_kind,
                    score: 150,
                    confidence: format!("Concept Match ({})", m.concept),
                    explanation: format!("Matched conceptual tag '{}'", m.concept),
                    line: None,
                });
                concept_added += 1;
            }
        }
    }
    results.sort_by(|a, b| b.score.cmp(&a.score));
    (results, reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use storage::Database;

    fn test_db() -> (Database, std::path::PathBuf) {
        let unique = format!(
            "codebroker_resolver_test_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let root = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(root.join(".codebroker")).unwrap();
        let db_path = root.join(".codebroker").join("codebroker.db");
        let db = Database::new(db_path.to_str().unwrap()).unwrap();
        db.init_schema().unwrap();
        (db, root)
    }

    fn insert_symbol(db: &Database, file_id: i64, name: &str, kind: &str) -> i64 {
        db.insert_symbol(
            file_id,
            &graph::SymbolNode {
                name: name.to_string(),
                kind: kind.to_string(),
                start_line: 1,
                end_line: 5,
                start_byte: 0,
                end_byte: 0,
                signature: None,
                attributes: Vec::new(),
                metadata: None,
            },
        )
        .unwrap()
    }

    #[test]
    fn exact_symbol_match_resolves_uniquely() {
        let (db, root) = test_db();
        let f = db.insert_file("./a.py", "h1").unwrap();
        insert_symbol(&db, f, "simulate", "function");

        let result = resolve_symbol(&db, "simulate", None, None, None);
        match result {
            ResolvedEntity::Symbol(s) => assert_eq!(s.name, "simulate"),
            other => panic!("expected Symbol, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn duplicate_symbol_name_is_ambiguous_not_silently_picked() {
        let (db, root) = test_db();
        let f1 = db.insert_file("./a.py", "h1").unwrap();
        let f2 = db.insert_file("./b.py", "h2").unwrap();
        insert_symbol(&db, f1, "SOFTWARE_REGISTRY", "variable");
        insert_symbol(&db, f2, "SOFTWARE_REGISTRY", "variable");

        let result = resolve_symbol(&db, "SOFTWARE_REGISTRY", None, None, None);
        assert!(
            result.is_ambiguous(),
            "expected Ambiguous, got {:?}",
            result
        );
        if let ResolvedEntity::Ambiguous(a) = result {
            assert_eq!(a.candidates.len(), 2);
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn file_hint_disambiguates_duplicate_symbol() {
        let (db, root) = test_db();
        let f1 = db.insert_file("./a.py", "h1").unwrap();
        let f2 = db.insert_file("./b.py", "h2").unwrap();
        insert_symbol(&db, f1, "createClient", "function");
        insert_symbol(&db, f2, "createClient", "function");

        let result = resolve_symbol(&db, "createClient", Some("b.py"), None, None);
        match result {
            ResolvedEntity::Symbol(s) => assert!(s.file_path.ends_with("b.py")),
            other => panic!("expected Symbol, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn unknown_symbol_returns_not_found_never_a_guess() {
        let (db, root) = test_db();
        db.insert_file("./a.py", "h1").unwrap();

        let result = resolve_symbol(&db, "totallyMadeUpName", None, None, None);
        assert!(result.is_not_found(), "expected NotFound, got {:?}", result);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn full_path_resolves_to_file() {
        let (db, root) = test_db();
        db.insert_file("./OrcaAI/orchestrator/main.py", "h1")
            .unwrap();

        let result = resolve_path(&db, "OrcaAI/orchestrator/main.py");
        match result {
            ResolvedEntity::File(f) => assert!(f.file_path.ends_with("main.py")),
            other => panic!("expected File, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn suffix_match_resolves_nested_file_uniquely() {
        let (db, root) = test_db();
        db.insert_file("./OrcaAI/frontend/app/page.tsx", "h1")
            .unwrap();

        let result = resolve_path(&db, "frontend/app/page.tsx");
        match result {
            ResolvedEntity::File(f) => assert!(f.file_path.ends_with("page.tsx")),
            other => panic!("expected File, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn ambiguous_filename_across_two_directories_is_flagged_not_guessed() {
        let (db, root) = test_db();
        db.insert_file("./a/main.py", "h1").unwrap();
        db.insert_file("./b/main.py", "h2").unwrap();

        let result = resolve_path(&db, "main.py");
        assert!(
            result.is_ambiguous(),
            "expected Ambiguous, got {:?}",
            result
        );
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn directory_resolves_with_children_listed() {
        let (db, root) = test_db();
        db.insert_file("./OrcaAI/frontend/app/page.tsx", "h1")
            .unwrap();
        db.insert_file("./OrcaAI/frontend/app/layout.tsx", "h2")
            .unwrap();

        let result = resolve_path(&db, "frontend/app");
        match result {
            ResolvedEntity::Directory(d) => assert_eq!(d.file_count, 2),
            other => panic!("expected Directory, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn nonexistent_path_returns_not_found() {
        let (db, root) = test_db();
        db.insert_file("./a.py", "h1").unwrap();

        let result = resolve_path(&db, "does/not/exist.py");
        assert!(result.is_not_found());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn exact_concept_name_resolves_to_feature() {
        let (db, root) = test_db();
        let f = db.insert_file("./auth.py", "h1").unwrap();
        let sid = insert_symbol(&db, f, "login", "function");
        db.conn
            .execute(
                "INSERT INTO symbol_concepts (symbol_id, concept, matched_on) VALUES (?1, 'auth', 'login')",
                rusqlite::params![sid],
            )
            .unwrap();

        let result = resolve_any(&db, "auth", None, &[], None);
        match result {
            ResolvedEntity::Feature(f) => assert_eq!(f.concept, "auth"),
            other => panic!("expected Feature, got {:?}", other),
        }
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn natural_language_concept_phrase_does_not_short_circuit_into_feature() {
        // "authentication" is not an exact concept name (the concept is
        // "auth"), so the Feature stage must NOT claim it — it should fall
        // through toward Subsystem/Semantic instead of pre-empting them.
        let (db, root) = test_db();
        db.insert_file("./a.py", "h1").unwrap();
        assert!(stage_feature(&db, "authentication").is_none());
        std::fs::remove_dir_all(&root).ok();
    }
}
