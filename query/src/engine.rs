use storage::Database;
use rusqlite::params;
use std::time::Instant;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SymbolCandidate {
    pub name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: i64,
}

/// Lists every row matching `name` exactly, with no LIMIT 1 collapse. Used as
/// a pre-flight check by tools that take a bare symbol name and would
/// otherwise silently pick whichever row SQLite happens to return first
/// (insertion order) when a common name like "GET" or "handler" is defined
/// in many files — that's a real correctness risk, not just a precision nit,
/// because the caller has no way to know they got the wrong file's symbol.
pub fn find_symbol_candidates(db: &Database, name: &str) -> Result<Vec<SymbolCandidate>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT symbols.name, symbols.kind, files.path, symbols.start_line
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.name = ?1"
    )?;
    let mut rows = stmt.query(params![name])?;
    let mut out = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(2)?;
        out.push(SymbolCandidate {
            name: row.get(0)?,
            kind: row.get(1)?,
            file_path: db.resolve_path(&path),
            start_line: row.get(3)?,
        });
    }
    Ok(out)
}

pub fn find_dependents(db: &Database, target_symbol_name: &str) ->
Result<Vec<String>, rusqlite::Error> {
    let parent_class = if target_symbol_name.contains('.') {
        target_symbol_name.split('.').next().unwrap_or(target_symbol_name)
    } else {
        target_symbol_name
    };

    let mut stmt = db.conn.prepare(
        "SELECT files.path 
        FROM edges
        JOIN symbols ON edges.target_symbol_id = symbols.id
        JOIN files ON edges.source_file_id = files.id
        WHERE (symbols.name = ?1 OR symbols.name = ?2) AND edges.kind = 'imports'"
    )?;

    let mut rows = stmt.query(params![target_symbol_name, parent_class])?;
    let mut dependents = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        dependents.push(db.resolve_path(&path));
    }
    Ok(dependents)
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct DirectoryStats {
    pub path: String,
    pub file_count: i64,
    pub symbol_count: i64,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ProjectOverview {
    pub files: i64,
    pub symbols: i64,
    pub edges: i64,
    pub languages: std::collections::HashMap<String, i64>,
    /// Per-directory file AND symbol counts. `symbol_count` is the signal for
    /// "is this directory worth a search_codebase/find_symbol call" — a
    /// directory with many files but near-zero symbols (e.g. assets, generated
    /// output) isn't worth querying even though it shows up in the file count.
    pub top_level_directories: Vec<DirectoryStats>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub score: i32,
    pub confidence: String,
    pub explanation: String,
    /// Line number for content/text matches. Absent for symbol/file matches.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<i64>,
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let len_a = a.len();
    let len_b = b.len();

    let mut dp = vec![vec![0; len_b + 1]; len_a + 1];

    for i in 0..=len_a { dp[i][0] = i; }
    for j in 0..=len_b { dp[0][j] = j; }

    for i in 1..=len_a {
        for j in 1..=len_b {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[len_a][len_b]
}

fn deterministic_query_expansion(keyword: &str) -> Vec<String> {
    let query_lower = keyword.to_lowercase();
    let stopwords = ["a", "an", "the", "and", "or", "in", "on", "at", "to", "for", "with", "by", "of", "from", "system", "feature", "code", "run", "user", "users", "use", "using", "used"];
    let mut tokens: Vec<String> = query_lower.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty() && !stopwords.contains(s))
        .map(|s| s.to_string())
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn token_weight(token: &str) -> i32 {
    let high_value = ["notification", "notifications", "judge", "awareness", "cursor", "execution", "auth", "authentication", "collaborate", "collaboration", "sync", "presence", "websocket", "realtime", "api"];
    if high_value.contains(&token) {
        return 300;
    }
    100
}

/// Caps how many indexed files get their on-disk content scanned per `text`/`both`
/// search, so a single call can't degrade into an O(repo size) full-text grep on a
/// huge monorepo. Combined with `path_scope`, this keeps the worst case bounded.
const MAX_TEXT_SCAN_FILES: usize = 2000;
const MAX_TEXT_MATCHES: usize = 50;

/// Search mode for `search_symbols`. `Symbol` only matches indexed symbol/file
/// names (fast, but misses string literals, comments, and config values).
/// `Text` greps the raw file content of indexed files (slower, catches anything
/// `Symbol` misses). `Both` runs symbol matching first and falls back to text
/// search if that yields nothing, merging whatever is found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Symbol,
    Text,
    Both,
}

impl From<&str> for SearchMode {
    fn from(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "text" => SearchMode::Text,
            "both" => SearchMode::Both,
            _ => SearchMode::Symbol,
        }
    }
}

/// Maps a `SearchResult.confidence` string to a coarse 1-3 level for
/// `min_confidence` filtering. Confidence strings always lead with "High"/
/// "Medium"/"Low" except the file-path weak-token bucket ("File Path Match"),
/// which is intentionally classified as Low rather than left unrecognized —
/// under-classifying is safer here than accidentally letting a weak match
/// through a `min_confidence: "medium"` filter.
fn confidence_level(confidence: &str) -> u8 {
    if confidence.starts_with("High") { 3 }
    else if confidence.starts_with("Medium") { 2 }
    else { 1 }
}

/// Parses the `min_confidence` tool param ("high" | "medium" | "low") into the
/// same 1-3 scale as `confidence_level`. Unrecognized or absent values mean
/// "no filtering" (0), preserving the default flat-list behavior.
fn min_confidence_level(min_confidence: Option<&str>) -> u8 {
    match min_confidence.map(|s| s.to_lowercase()).as_deref() {
        Some("high") => 3,
        Some("medium") => 2,
        Some("low") => 1,
        _ => 0,
    }
}

pub struct RankingFeatures {
    pub semantic_similarity: f64, // -1.0 to 1.0 (cosine sim)
    pub lexical_score: i32,
    pub path_score: i32,
    pub pagerank: f64,
    pub fan_in: i64,
    pub fan_out: i64,
    pub is_entrypoint: bool,
    pub is_callable: bool,
    pub is_local: bool,
}

pub fn compute_retrieval_score(features: &RankingFeatures) -> i32 {
    let mut base_score = features.lexical_score + features.path_score;
    // Base semantic score contribution. If sim > 0.0, we add up to 2000 points.
    // This makes semantic score comparable to a strong exact match (1000).
    if features.semantic_similarity > 0.0 {
        base_score += (features.semantic_similarity * 2000.0) as i32;
    }
    
    // Structural Multipliers
    let mut multiplier = 1.0;
    if features.is_entrypoint {
        multiplier *= 1.5;
    } else if features.is_callable {
        multiplier *= 1.1;
    }
    
    if features.is_local {
        multiplier *= 0.3; // Heavy penalty for locals
    }
    
    // Graph Centrality Multiplier (pagerank typically around 1.0, could be up to 10.0+ for central nodes)
    let pr_factor = 1.0 + (features.pagerank * 0.1);
    multiplier *= pr_factor.min(2.5); // Cap multiplier at 2.5
    
    // Edge count minor boosts
    let edge_bonus = (features.fan_in * 2) + features.fan_out;
    
    ((base_score as f64 * multiplier) as i32) + edge_bonus as i32
}

fn search_symbol_names(
    db: &Database,
    query_lower: &str,
    query_tokens: &[String],
    path_scope: Option<&str>,
    query_vector: Option<&[f32]>,
) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let mut results = Vec::new();

    // 1. Fetch all symbol embeddings first if we have a query vector
    let mut symbol_embeddings: std::collections::HashMap<i64, Vec<f32>> = std::collections::HashMap::new();
    if let Some(_) = query_vector {
        let mut embed_stmt = db.conn.prepare("SELECT symbol_id, embedding FROM symbol_embeddings")?;
        let mut embed_rows = embed_stmt.query([])?;
        while let Some(row) = embed_rows.next()? {
            let s_id: i64 = row.get(0)?;
            let blob: Vec<u8> = row.get(1)?;
            let vec = storage::blob_to_embedding(&blob);
            symbol_embeddings.insert(s_id, vec);
        }
    }

    let query_str = "
        SELECT 
            files.path, 
            symbols.name, 
            symbols.kind,
            f.fan_in,
            f.fan_out,
            symbols.id,
            f.pagerank,
            f.is_entrypoint,
            f.is_callable,
            f.is_local
        FROM symbols
        JOIN files ON symbols.file_id = files.id
        LEFT JOIN symbol_features f ON symbols.id = f.symbol_id
    ";
    let mut stmt = db.conn.prepare(query_str)?;
    let mut rows = stmt.query([])?;

    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let in_edges: i64 = row.get(3).unwrap_or(0);
        let out_edges: i64 = row.get(4).unwrap_or(0);
        let s_id: i64 = row.get(5)?;
        let pagerank: f64 = row.get(6).unwrap_or(0.0);
        let is_entrypoint: bool = row.get(7).unwrap_or(false);
        let is_callable: bool = row.get(8).unwrap_or(false);
        let is_local: bool = row.get(9).unwrap_or(false);

        let name_lower = name.to_lowercase();
        let path_lower = path.to_lowercase();

        let mut name_score = 0;
        let mut path_score = 0;

        // 1. Symbol Name Match
        let mut expl = Vec::new();

        if name_lower == query_lower { name_score += 1000; expl.push(format!("Exact lexical match")); }
        else if name_lower.starts_with(query_lower) { name_score += 500; expl.push(format!("Prefix lexical match")); }
        else if name_lower.contains(query_lower) { name_score += 250; expl.push(format!("Substring lexical match")); }

        for token in query_tokens {
            let weight = token_weight(token);
            if name_lower == *token { name_score += weight; expl.push(format!("Token match '{}'", token)); }
            else {
                let dist = levenshtein(&name_lower, token);
                if dist <= 1 && token.len() > 3 {
                    name_score += weight / 2; expl.push(format!("Fuzzy match '{}'", token));
                } else if dist == 2 && token.len() > 5 {
                    name_score += weight / 4; expl.push(format!("Weak fuzzy match '{}'", token));
                }
            }
        }

        // 2. Full Path Match
        if path_lower.contains(query_lower) { path_score += 200; expl.push(format!("Path contains query")); }
        for token in query_tokens {
            let weight = token_weight(token);
            if path_lower.contains(token) { path_score += weight / 2; expl.push(format!("Path contains '{}'", token)); }
        }

        let mut semantic_sim = -1.0;
        if let Some(q_vec) = query_vector {
            if let Some(s_vec) = symbol_embeddings.get(&s_id) {
                semantic_sim = storage::cosine_similarity(q_vec, s_vec);
                if semantic_sim > 0.25 {
                    expl.push(format!("Semantic similarity {:.2}", semantic_sim));
                }
            }
        }

        if name_score == 0 && path_score == 0 && semantic_sim < 0.25 { 
            continue;
        }

        // 4. Production Path Boosts
        let path_parts: Vec<&str> = path_lower.split('/').collect();
        if path_parts.contains(&"app") || path_parts.contains(&"src") || path_parts.contains(&"server") || path_parts.contains(&"backend") || path_parts.contains(&"core") || path_parts.contains(&"api") {
            path_score += 150;
            expl.push(format!("Production path bonus"));
        }

        // 5. Scratch/Test Penalties
        if path_parts.contains(&"scratch") || path_parts.contains(&"sandbox") || path_parts.contains(&"tmp") || path_parts.contains(&"examples") || path_parts.contains(&"test") || path_parts.contains(&"tests") {
            path_score -= 300;
            expl.push(format!("Test/scratch penalty"));
        }

        let features = RankingFeatures {
            semantic_similarity: semantic_sim as f64,
            lexical_score: name_score,
            path_score,
            pagerank,
            fan_in: in_edges,
            fan_out: out_edges,
            is_entrypoint,
            is_callable,
            is_local,
        };

        let final_score = compute_retrieval_score(&features);

        let confidence = if semantic_sim > 0.35 { "High (Semantic Match)".to_string() }
        else if name_score >= 1000 { "High (Exact Match)".to_string() }
        else if name_score >= 500 { "High (Prefix Match)".to_string() }
        else if semantic_sim > 0.25 { "Medium (Semantic Match)".to_string() }
        else if name_score >= 250 { "Medium (Contains Match)".to_string() }
        else if name_score >= 100 { "Medium (Token Match)".to_string() }
        else if name_score >= 50 { "Low (Fuzzy Match)".to_string() }
        else { "Low (Weak Fuzzy)".to_string() };

        if final_score > 0 {
            results.push(SearchResult { path: db.resolve_path(&path), name, kind, score: final_score, confidence, explanation: expl.join(", "), line: None });
        }
    }

    // 6. Ambiguity Resolution (Group by exact name, sort, demote duplicates)
    // We group by symbol name to treat ambiguity as a ranking problem.
    let mut groups: std::collections::HashMap<String, Vec<SearchResult>> = std::collections::HashMap::new();
    for res in results.into_iter() {
        groups.entry(res.name.clone()).or_default().push(res);
    }
    
    let mut resolved_results = Vec::new();
    for (_name, mut group) in groups {
        group.sort_by(|a, b| b.score.cmp(&a.score));
        
        // The strongest candidate keeps its score. 
        // Remaining candidates are demoted to ensure the canonical definition sits on top.
        for (i, mut res) in group.into_iter().enumerate() {
            if i > 0 {
                res.score = (res.score as f64 * 0.7) as i32; // Penalty for ambiguous duplicate
                if res.confidence.starts_with("High") {
                    res.confidence = res.confidence.replace("High", "Medium");
                }
                res.explanation.push_str(" (Ambiguous duplicate demoted)");
            }
            resolved_results.push(res);
        }
    }
    
    let mut results = resolved_results;

    // 7. Check isolated files table
    let mut file_stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut file_rows = file_stmt.query([])?;
    while let Some(row) = file_rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }

        let path_lower = path.to_lowercase();
        let filename = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_lowercase();
        
        let mut expl = Vec::new();
        let mut base_score = 0;

        if filename == query_lower { base_score += 800; expl.push(format!("Exact file match")); }
        else if filename.contains(query_lower) { base_score += 250; expl.push(format!("File substring match")); }
        
        if path_lower.contains(query_lower) && !filename.contains(query_lower) { base_score += 100; expl.push(format!("Path contains query")); }

        for token in query_tokens {
            let weight = token_weight(token);
            if filename == *token { base_score += weight; expl.push(format!("Token file match '{}'", token)); }
            else if filename.contains(token) { base_score += weight / 2; expl.push(format!("File fuzzy match '{}'", token)); }
            else if path_lower.contains(token) { base_score += weight / 4; expl.push(format!("Path token match '{}'", token)); }
        }

        if base_score == 0 {
            continue;
        }
        
        let mut relevance_score = base_score;

        let path_parts: Vec<&str> = path_lower.split('/').collect();
        if path_parts.contains(&"app") || path_parts.contains(&"src") || path_parts.contains(&"server") || path_parts.contains(&"backend") || path_parts.contains(&"core") || path_parts.contains(&"api") {
            relevance_score += 150;
            expl.push(format!("Production path bonus"));
        }
        if path_parts.contains(&"scratch") || path_parts.contains(&"sandbox") || path_parts.contains(&"tmp") || path_parts.contains(&"examples") || path_parts.contains(&"test") || path_parts.contains(&"tests") {
            relevance_score -= 300;
            expl.push(format!("Test/scratch penalty"));
        }

        if relevance_score > 0 {
            let confidence = if relevance_score >= 800 { "High (Exact File Match)".to_string() }
            else if relevance_score >= 250 { "Medium (File Substring Match)".to_string() }
            else if relevance_score >= 100 { "Low (Token File Match)".to_string() }
            else { "Low (File Path Match)".to_string() };

            let final_score = relevance_score;
            results.push(SearchResult {
                path: db.resolve_path(&path),
                name: std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_string(),
                kind: "file".to_string(),
                score: final_score,
                confidence,
                explanation: expl.join(", "),
                line: None,
            });
        }
    }

    Ok(results)
}

/// True if `haystack_lower` contains `needle_lower` at a position bounded by
/// non-identifier characters (or string edges) on both sides — i.e. a whole
/// "word" match, not a substring inside a longer identifier. Both inputs
/// must already be lowercased by the caller. This is what distinguishes a
/// search for "port" from incorrectly matching inside "export" or "import".
fn contains_whole_word(haystack_lower: &str, needle_lower: &str) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    let bytes = haystack_lower.as_bytes();
    let needle_bytes = needle_lower.as_bytes();
    let is_word_byte = |b: u8| b.is_ascii_alphanumeric() || b == b'_';

    let mut start = 0;
    while let Some(rel_pos) = haystack_lower[start..].find(needle_lower) {
        let pos = start + rel_pos;
        let before_ok = pos == 0 || !is_word_byte(bytes[pos - 1]);
        let end = pos + needle_bytes.len();
        let after_ok = end >= bytes.len() || !is_word_byte(bytes[end]);
        if before_ok && after_ok {
            return true;
        }
        start = pos + 1;
        if start >= haystack_lower.len() {
            break;
        }
    }
    false
}

/// Literal/substring scan over the raw content of indexed files. Unlike symbol
/// search, this catches string literals, comments, and config values — the
/// case that drove this addition (a keyword like "leetcode" appearing only in
/// a string, never as a symbol name). `whole_word` requires non-identifier
/// boundaries around the match (fixes false positives like "port" matching
/// inside "export"/"import") at the cost of missing intentional substring
/// searches — it defaults to off so existing substring behavior is preserved.
fn search_file_contents(
    db: &Database,
    query_lower: &str,
    path_scope: Option<&str>,
    whole_word: bool,
) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let mut results = Vec::new();
    let mut file_stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut file_rows = file_stmt.query([])?;

    let mut files_scanned = 0usize;
    while let Some(row) = file_rows.next()? {
        if files_scanned >= MAX_TEXT_SCAN_FILES || results.len() >= MAX_TEXT_MATCHES {
            break;
        }
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }

        let abs_path = db.resolve_path(&path);
        let content = match std::fs::read_to_string(&abs_path) {
            Ok(c) => c,
            Err(_) => continue, // binary or unreadable; skip rather than fail the whole search
        };
        files_scanned += 1;

        for (idx, line) in content.lines().enumerate() {
            if results.len() >= MAX_TEXT_MATCHES {
                break;
            }
            let line_lower = line.to_lowercase();
            let is_match = if whole_word {
                contains_whole_word(&line_lower, query_lower)
            } else {
                line_lower.contains(query_lower)
            };
            if is_match {
                let trimmed = line.trim();
                let preview = if trimmed.len() > 160 {
                    format!("{}...", trimmed.chars().take(160).collect::<String>())
                } else {
                    trimmed.to_string()
                };
                results.push(SearchResult {
                    path: abs_path.clone(),
                    name: preview,
                    kind: "text_match".to_string(),
                    score: 200,
                    confidence: "Medium (Content Match)".to_string(),
                    explanation: format!("Matched literal text in file"),
                    line: Some((idx + 1) as i64),
                });
            }
        }
    }

    Ok(results)
}

pub fn search_symbols(
    db: &Database,
    keyword: &str,
    semantic_tokens: &[String],
    query_vector: Option<&[f32]>,
    llm_used: bool,
    path_scope: Option<&str>,
    mode: SearchMode,
    whole_word: bool,
    min_confidence: Option<&str>,
) -> Result<(Vec<SearchResult>, Option<String>), rusqlite::Error> {
    let start_time = Instant::now();

    let query_lower = keyword.to_lowercase();
    let mut query_tokens = deterministic_query_expansion(keyword);

    if llm_used && query_vector.is_none() {
        return Ok((Vec::new(), Some("Semantic search unavailable. Workspace indexed without embeddings.".to_string())));
    }

    // Inject the AI-generated semantic synonyms into our token pool
    for st in semantic_tokens {
        if !query_tokens.contains(st) {
            query_tokens.push(st.clone());
        }
    }

    let mut results = match mode {
        SearchMode::Symbol => search_symbol_names(db, &query_lower, &query_tokens, path_scope, query_vector)?,
        SearchMode::Text => search_file_contents(db, &query_lower, path_scope, whole_word)?,
        SearchMode::Both => {
            let mut symbol_results = search_symbol_names(db, &query_lower, &query_tokens, path_scope, query_vector)?;
            if symbol_results.is_empty() {
                symbol_results.extend(search_file_contents(db, &query_lower, path_scope, whole_word)?);
            }
            symbol_results
        }
    };

    let pre_filter_count = results.len();
    let required_level = min_confidence_level(min_confidence);
    if required_level > 0 {
        results.retain(|r| confidence_level(&r.confidence) >= required_level);
    }
    let filtered_out_by_confidence = required_level > 0 && results.is_empty() && pre_filter_count > 0;

    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(50);

    // Sprint 6: Search Analytics
    let latency_ms = start_time.elapsed().as_millis() as i64;
    let result_count = results.len() as i64;
    let top_result = results.first().map(|r| r.name.clone());
    let fallback_used = !llm_used;
    let search_mode_label = if llm_used { "semantic_boost" } else { "deterministic" };

    let _ = db.conn.execute(
        "INSERT INTO search_events (query, result_count, latency_ms, fallback_used, llm_used, top_result, search_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![keyword, result_count, latency_ms, fallback_used, llm_used, top_result, search_mode_label]
    );

    let reason = if filtered_out_by_confidence {
        Some(format!(
            "Found {} match(es) for \"{}\" but none met min_confidence: \"{}\". Lower min_confidence or omit it to see them.",
            pre_filter_count, keyword, min_confidence.unwrap_or("")
        ))
    } else if results.is_empty() {
        let scoped_symbol_count: i64 = if let Some(scope) = path_scope {
            let pattern = format!("%{}%", scope);
            db.conn.query_row(
                "SELECT COUNT(*) FROM symbols JOIN files ON symbols.file_id = files.id WHERE files.path LIKE ?1",
                params![pattern],
                |r| r.get(0),
            ).unwrap_or(0)
        } else {
            db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0)
        };
        let mode_hint = match mode {
            SearchMode::Symbol => " Try mode: \"text\" or \"both\" to also search file content/string literals, not just symbol names.".to_string(),
            SearchMode::Text => " Try mode: \"symbol\" or \"both\" to also search indexed symbol names.".to_string(),
            SearchMode::Both => String::new(),
        };
        let whole_word_hint = if whole_word {
            " whole_word: true is on — if you meant a substring match (e.g. inside a longer identifier), retry with whole_word: false."
        } else {
            ""
        };
        Some(format!(
            "No matches for \"{}\" in mode \"{}\"; {} indexed symbols in scope.{}{}",
            keyword,
            match mode { SearchMode::Symbol => "symbol", SearchMode::Text => "text", SearchMode::Both => "both" },
            scoped_symbol_count,
            mode_hint,
            whole_word_hint,
        ))
    } else {
        None
    };

    Ok((results, reason))
}

pub fn find_symbol_exact(db: &Database, name: &str, context_lines: usize) -> Result<Vec<(String, String, i64, String)>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 5"
    )?;
    
    let mut rows = stmt.query(params![name])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let start_line: i64 = row.get(2)?;
        let end_line: i64 = row.get(3).unwrap_or(start_line);
        
        let abs_path = db.resolve_path(&path);
        let mut preview = String::new();
        if let Ok(content) = std::fs::read_to_string(&abs_path) {
            let lines: Vec<&str> = content.lines().collect();
            let start = (start_line.saturating_sub(context_lines as i64 + 1)).max(0) as usize;
            let end = (end_line + context_lines as i64).min(lines.len() as i64) as usize;
            if start < lines.len() {
                preview = lines[start..end].join("\n");
            }
        }
        
        results.push((abs_path, kind, start_line, preview));
    }
    Ok(results)
}

/// Returns ranked matches as tuples of
/// `(name, abs_path, kind, start_line, preview, score)`. `name` is the matched
/// symbol's own name (NOT the query) — callers use it to separate exact hits
/// from fuzzy ones, since `find_symbol` deliberately blends both by score.
pub fn find_symbol(db: &Database, query: &str, context_lines: usize, path_scope: Option<&str>) -> Result<Vec<(String, String, String, i64, String, i32)>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind, symbols.start_line, symbols.end_line
         FROM symbols
         JOIN files ON symbols.file_id = files.id"
    )?;

    let mut rows = stmt.query([])?;
    let mut candidates = Vec::new();
    let query_lower = query.to_lowercase();

    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        let start_line: i64 = row.get(3)?;
        let end_line: i64 = row.get(4).unwrap_or(start_line);
        
        let name_lower = name.to_lowercase();
        let mut score = 0;
        
        if name_lower == query_lower {
            score += 1000;
        } else if name_lower.starts_with(&query_lower) {
            score += 500;
        } else if name_lower.contains(&query_lower) {
            score += 250;
        } else {
            let dist = levenshtein(&name_lower, &query_lower);
            if dist <= 1 && query_lower.len() > 3 {
                score += 100;
            } else if dist == 2 && query_lower.len() > 5 {
                score += 50;
            }
        }
        
        if score > 0 {
            candidates.push((score, path, name, kind, start_line, end_line));
        }
    }
    
    candidates.sort_by(|a, b| b.0.cmp(&a.0));
    candidates.truncate(10); // Return top 10 best matches
    
    let mut results = Vec::new();
    for (score, path, name, kind, start_line, end_line) in candidates {
        let abs_path = db.resolve_path(&path);
        // context_lines: 0 is documented as "existence-only" — skip the disk
        // read and source preview entirely so a yes/no location check doesn't
        // pay for source bytes the caller didn't ask for.
        let mut preview = String::new();
        if context_lines > 0 {
            if let Ok(content) = std::fs::read_to_string(&abs_path) {
                let lines: Vec<&str> = content.lines().collect();
                let start = (start_line.saturating_sub(context_lines as i64 + 1)).max(0) as usize;
                let end = (end_line + context_lines as i64).min(lines.len() as i64) as usize;
                if start < lines.len() {
                    preview = lines[start..end].join("\n");
                }
            }
        }
        results.push((name, abs_path, kind, start_line, preview, score));
    }

    Ok(results)
}

pub fn build_project_overview(db: &Database) -> Result<ProjectOverview, rusqlite::Error> {
    build_project_overview_scoped(db, None)
}

/// Like `build_project_overview`, but when `path_scope` is given, every count
/// (files/symbols/edges/languages/directories) is restricted to files whose
/// path contains that substring. This is what makes `repository_stats`
/// usable as a pre-check ("is this one directory worth querying") instead of
/// only ever reporting global totals.
pub fn build_project_overview_scoped(db: &Database, path_scope: Option<&str>) -> Result<ProjectOverview, rusqlite::Error> {
    let mut file_stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut file_rows = file_stmt.query([])?;
    let mut file_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut total_files: i64 = 0;
    let mut languages: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    while let Some(row) = file_rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        total_files += 1;
        if let Some(ext) = std::path::Path::new(&path).extension().and_then(|e| e.to_str()) {
            *languages.entry(ext.to_string()).or_insert(0) += 1;
        }
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let p_str = parent.to_string_lossy().to_string();
            *file_counts.entry(p_str).or_insert(0) += 1;
        }
    }

    let mut sym_stmt = db.conn.prepare(
        "SELECT files.path FROM symbols JOIN files ON symbols.file_id = files.id"
    )?;
    let mut sym_rows = sym_stmt.query([])?;
    let mut symbol_counts: std::collections::HashMap<String, i64> = std::collections::HashMap::new();
    let mut total_symbols: i64 = 0;
    while let Some(row) = sym_rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }
        total_symbols += 1;
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let p_str = parent.to_string_lossy().to_string();
            *symbol_counts.entry(p_str).or_insert(0) += 1;
        }
    }

    let total_edges: i64 = if let Some(scope) = path_scope {
        let pattern = format!("%{}%", scope);
        db.conn.query_row(
            "SELECT COUNT(*) FROM edges JOIN files ON edges.source_file_id = files.id WHERE files.path LIKE ?1",
            params![pattern],
            |r| r.get(0),
        ).unwrap_or(0)
    } else {
        db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0)
    };

    let mut dirs_vec: Vec<DirectoryStats> = file_counts.into_iter().map(|(path, file_count)| {
        let symbol_count = *symbol_counts.get(&path).unwrap_or(&0);
        DirectoryStats { path, file_count, symbol_count }
    }).collect();
    dirs_vec.sort_by(|a, b| b.file_count.cmp(&a.file_count));

    Ok(ProjectOverview {
        files: total_files,
        symbols: total_symbols,
        edges: total_edges,
        languages,
        top_level_directories: dirs_vec.into_iter().take(20).collect(),
    })
}

#[cfg(test)]
mod min_confidence_tests {
    use super::*;

    fn setup_fixture() -> Database {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();

        // "room" matches its own name exactly -> High (Exact Match).
        // "ChatRoom" contains "room" but doesn't start with it -> Medium (Contains Match).
        // "roof" is a 1-edit-distance fuzzy match with no substring overlap -> Low (Fuzzy Match).
        let file_id = db.insert_file("components.ts", "fixturehash").unwrap();
        for name in ["room", "ChatRoom", "roof"] {
            db.insert_symbol(file_id, &graph::SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 1,
                start_byte: 0,
                end_byte: 0,
                signature: None, attributes: Vec::new(), metadata: None,
            }).unwrap();
        }
        db
    }

    #[test]
    fn confidence_level_maps_known_prefixes() {
        assert_eq!(confidence_level("High (Exact Match)"), 3);
        assert_eq!(confidence_level("Medium (Contains Match)"), 2);
        assert_eq!(confidence_level("Low (Fuzzy Match)"), 1);
        // Unrecognized strings (e.g. "File Path Match") classify as Low
        // rather than being silently excluded from every filter tier.
        assert_eq!(confidence_level("File Path Match"), 1);
    }

    #[test]
    fn min_confidence_level_parses_known_tiers_case_insensitively() {
        assert_eq!(min_confidence_level(Some("high")), 3);
        assert_eq!(min_confidence_level(Some("HIGH")), 3);
        assert_eq!(min_confidence_level(Some("medium")), 2);
        assert_eq!(min_confidence_level(Some("low")), 1);
        assert_eq!(min_confidence_level(None), 0);
        assert_eq!(min_confidence_level(Some("garbage")), 0);
    }

    #[test]
    fn default_search_returns_all_confidence_tiers_unfiltered() {
        let db = setup_fixture();
        let (results, _) = search_symbols(&db, "room", &[], None, false, None, SearchMode::Symbol, false, None).unwrap();
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"room"));
        assert!(names.contains(&"ChatRoom"));
        assert!(names.contains(&"roof"), "default (no min_confidence) must not drop any tier, got: {:?}", names);
    }

    #[test]
    fn min_confidence_high_drops_medium_and_low_noise() {
        let db = setup_fixture();
        let (results, _) = search_symbols(&db, "room", &[], None, false, None, SearchMode::Symbol, false, Some("high")).unwrap();
        let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, vec!["room"], "min_confidence: \"high\" must keep only the exact match");
    }

    #[test]
    fn min_confidence_filtering_does_not_change_result_count_when_unset() {
        let db = setup_fixture();
        let (unfiltered, _) = search_symbols(&db, "room", &[], None, false, None, SearchMode::Symbol, false, None).unwrap();
        let (explicit_low, _) = search_symbols(&db, "room", &[], None, false, None, SearchMode::Symbol, false, Some("low")).unwrap();
        assert_eq!(unfiltered.len(), explicit_low.len(), "every tier here is >= Low, so min_confidence: \"low\" should be a no-op");
    }
}

#[cfg(test)]
mod find_symbol_tests {
    use super::*;

    // #3 — exact_vs_fuzzy_fixture: find_symbol now returns the matched symbol's
    // own NAME (first tuple field), which the MCP layer uses to split
    // exact_matches from fuzzy_matches. The exact hit must also outrank fuzzy.
    #[test]
    fn find_symbol_returns_names_and_ranks_exact_first() {
        let db = Database::new(":memory:").unwrap();
        db.init_schema().unwrap();
        let file_id = db.insert_file("handlers.ts", "h").unwrap();
        for name in ["GET", "GetStarted", "GetGraphHelpers"] {
            db.insert_symbol(file_id, &graph::SymbolNode {
                name: name.to_string(),
                kind: "function".to_string(),
                start_line: 1,
                end_line: 1,
                start_byte: 0,
                end_byte: 0,
                signature: None, attributes: Vec::new(), metadata: None,
            }).unwrap();
        }

        // context_lines = 0 -> no disk reads needed for previews.
        let results = find_symbol(&db, "GET", 0, None).unwrap();
        let names: Vec<&str> = results.iter().map(|(name, ..)| name.as_str()).collect();
        assert!(names.contains(&"GET"));
        assert!(names.contains(&"GetStarted"));

        // Exact match must be first (highest score).
        assert_eq!(results.first().map(|(n, ..)| n.as_str()), Some("GET"));

        // The classification rule the MCP handler applies: exactly one exact.
        let exact: Vec<&str> = names.iter().filter(|n| n.to_lowercase() == "get").copied().collect();
        assert_eq!(exact, vec!["GET"], "only GET is an exact match");
    }
}
