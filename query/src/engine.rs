use storage::Database;
use rusqlite::params;
use std::time::Instant;

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
pub struct ProjectOverview {
    pub files: i64,
    pub symbols: i64,
    pub edges: i64,
    pub languages: std::collections::HashMap<String, i64>,
    pub top_level_directories: Vec<(String, i64)>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub kind: String,
    pub score: i32,
    pub confidence: String,
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

pub fn search_symbols(
    db: &Database, 
    keyword: &str, 
    semantic_tokens: &[String], 
    llm_used: bool
) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let start_time = Instant::now();
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id"
    )?;
    
    let query_lower = keyword.to_lowercase();
    let mut query_tokens = deterministic_query_expansion(keyword);
    
    // Inject the AI-generated semantic synonyms into our token pool
    for st in semantic_tokens {
        if !query_tokens.contains(st) {
            query_tokens.push(st.clone());
        }
    }
    
    let mut results = Vec::new();

    // 0. check files table for matches
    let mut file_stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut file_rows = file_stmt.query([])?;
    while let Some(row) = file_rows.next()? {
        let path: String = row.get(0)?;
        let path_lower = path.to_lowercase();
        
        let filename = std::path::Path::new(&path).file_name().and_then(|n| n.to_str()).unwrap_or(&path).to_lowercase();

        let mut score = 0;
        if filename == query_lower { score += 800; }
        else if filename.contains(&query_lower) { score += 250; }
        
        for token in &query_tokens {
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
            });
        }
    }
    
    let mut rows = stmt.query([])?;
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        
        let name_lower = name.to_lowercase();
        let mut score = 0;

        if name_lower == query_lower {
            score += 1000;
        } else if name_lower.starts_with(&query_lower) {
            score += 500;
        } else if name_lower.contains(&query_lower) {
            score += 250;
        }

        for token in &query_tokens {
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
            
            results.push(SearchResult { path: db.resolve_path(&path), name, kind, score, confidence });
        }
    }
    
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(50);

    // Sprint 6: Search Analytics
    let latency_ms = start_time.elapsed().as_millis() as i64;
    let result_count = results.len() as i64;
    let top_result = results.first().map(|r| r.name.clone());
    let fallback_used = !llm_used;
    let search_mode = if llm_used { "semantic_boost" } else { "deterministic" };
    
    let _ = db.conn.execute(
        "INSERT INTO search_events (query, result_count, latency_ms, fallback_used, llm_used, top_result, search_mode)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![keyword, result_count, latency_ms, fallback_used, llm_used, top_result, search_mode]
    );

    Ok(results)
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

pub fn find_symbol(db: &Database, query: &str, context_lines: usize) -> Result<Vec<(String, String, i64, String, i32)>, rusqlite::Error> {
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
        let mut preview = String::new();
        if let Ok(content) = std::fs::read_to_string(&abs_path) {
            let lines: Vec<&str> = content.lines().collect();
            let start = (start_line.saturating_sub(context_lines as i64 + 1)).max(0) as usize;
            let end = (end_line + context_lines as i64).min(lines.len() as i64) as usize;
            if start < lines.len() {
                preview = lines[start..end].join("\n");
            }
        }
        results.push((abs_path, kind, start_line, preview, score));
    }
    
    Ok(results)
}

pub fn build_project_overview(db: &Database) -> Result<ProjectOverview, rusqlite::Error> {
    let stats = db.get_codebroker_stats()?;
    let total_symbols: i64 = db.conn.query_row("SELECT COUNT(*) FROM symbols", [], |r| r.get(0)).unwrap_or(0);
    let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);
    
    let mut stmt = db.conn.prepare("SELECT path FROM files")?;
    let mut rows = stmt.query([])?;
    let mut dirs = std::collections::HashMap::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        if let Some(parent) = std::path::Path::new(&path).parent() {
            let p_str = parent.to_string_lossy().to_string();
            *dirs.entry(p_str).or_insert(0) += 1;
        }
    }
    
    let mut dirs_vec: Vec<_> = dirs.into_iter().collect();
    dirs_vec.sort_by(|a, b| b.1.cmp(&a.1));
    
    Ok(ProjectOverview {
        files: stats.files_indexed,
        symbols: total_symbols,
        edges: total_edges,
        languages: stats.extensions,
        top_level_directories: dirs_vec.into_iter().take(20).collect(),
    })
}
