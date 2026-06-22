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
    let mut tokens: Vec<String> = query_lower.split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    tokens.sort();
    tokens.dedup();
    tokens
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

fn search_symbol_names(
    db: &Database,
    query_lower: &str,
    query_tokens: &[String],
    path_scope: Option<&str>,
) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let mut results = Vec::new();

    // 0. check files table for matches
    let mut file_stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut file_rows = file_stmt.query([])?;
    while let Some(row) = file_rows.next()? {
        let path: String = row.get(0)?;
        if let Some(scope) = path_scope {
            if !path.contains(scope) {
                continue;
            }
        }

        let filename = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_lowercase();

        let mut score = 0;
        if filename == query_lower { score += 800; }
        else if filename.contains(query_lower) { score += 250; }

        for token in query_tokens {
            if filename == *token { score += 100; }
            else if filename.contains(token) { score += 50; }
        }

        if score > 0 {
            let confidence = if score >= 800 { "High (Exact File Match)".to_string() }
            else if score >= 250 { "Medium (File Substring Match)".to_string() }
            else if score >= 100 { "Low (Token File Match)".to_string() }
            else { "File Path Match".to_string() };

            results.push(SearchResult {
                path: db.resolve_path(&path),
                name: std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_string(),
                kind: "file".to_string(),
                score,
                confidence,
                line: None,
            });
        }
    }

    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind
         FROM symbols
         JOIN files ON symbols.file_id = files.id"
    )?;
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

        let name_lower = name.to_lowercase();
        let mut score = 0;

        if name_lower == query_lower {
            score += 1000;
        } else if name_lower.starts_with(query_lower) {
            score += 500;
        } else if name_lower.contains(query_lower) {
            score += 250;
        }

        for token in query_tokens {
            if name_lower == *token {
                score += 100;
            } else {
                let dist = levenshtein(&name_lower, token);
                if dist <= 1 && token.len() > 3 {
                    score += 50;
                } else if dist == 2 && token.len() > 5 {
                    score += 25;
                }
            }
        }

        if score > 0 {
            let confidence = if score >= 1000 { "High (Exact Match)".to_string() }
            else if score >= 500 { "High (Prefix Match)".to_string() }
            else if score >= 250 { "Medium (Contains Match)".to_string() }
            else if score >= 100 { "Medium (Token Match)".to_string() }
            else if score >= 50 { "Low (Fuzzy Match)".to_string() }
            else { "Low (Weak Fuzzy)".to_string() };

            results.push(SearchResult { path: db.resolve_path(&path), name, kind, score, confidence, line: None });
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
    llm_used: bool,
    path_scope: Option<&str>,
    mode: SearchMode,
    whole_word: bool,
) -> Result<(Vec<SearchResult>, Option<String>), rusqlite::Error> {
    let start_time = Instant::now();

    let query_lower = keyword.to_lowercase();
    let mut query_tokens = deterministic_query_expansion(keyword);

    // Inject the AI-generated semantic synonyms into our token pool
    for st in semantic_tokens {
        if !query_tokens.contains(st) {
            query_tokens.push(st.clone());
        }
    }

    let mut results = match mode {
        SearchMode::Symbol => search_symbol_names(db, &query_lower, &query_tokens, path_scope)?,
        SearchMode::Text => search_file_contents(db, &query_lower, path_scope, whole_word)?,
        SearchMode::Both => {
            let mut symbol_results = search_symbol_names(db, &query_lower, &query_tokens, path_scope)?;
            if symbol_results.is_empty() {
                symbol_results.extend(search_file_contents(db, &query_lower, path_scope, whole_word)?);
            }
            symbol_results
        }
    };

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

    let reason = if results.is_empty() {
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

pub fn find_symbol(db: &Database, query: &str, context_lines: usize, path_scope: Option<&str>) -> Result<Vec<(String, String, i64, String, i32)>, rusqlite::Error> {
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
    for (score, path, _name, kind, start_line, end_line) in candidates {
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
        results.push((abs_path, kind, start_line, preview, score));
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
