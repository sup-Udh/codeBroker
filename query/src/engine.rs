use storage::Database;
use rusqlite::params;

pub fn find_dependents(db: &Database, target_symbol_name: &str) -> 
Result<Vec<String>, rusqlite::Error> {

    // pefroms sql joins across all the edges

    let mut  stmt = db.conn.prepare(
        "SELECT files.path 
        FROM edges
        JOIN symbols ON edges.target_symbol_id = symbols.id
        JOIN files ON edges.source_file_id = files.id
        WHERE symbols.name = ?1 and edges.kind = 'imports'"

    )?;


    let mut rows = stmt.query(params![target_symbol_name])?;
    let mut dependents = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        dependents.push(path);
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
}

fn levenshtein(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let len_a = a.len();
    let len_b = b.len();

    let mut dp = vec![vec![0; len_b + 1]; len_a + 1];

    for i in 0..=len_a {
        dp[i][0] = i;
    }
    for j in 0..=len_b {
        dp[0][j] = j;
    }

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

pub fn search_symbols(db: &Database, keyword: &str, semantic_tokens: &[String]) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id"
    )?;
    
    let query_lower = keyword.to_lowercase();
    let mut query_tokens: Vec<String> = query_lower.split_whitespace().map(|s| s.to_string()).collect();
    
    // Inject the AI-generated semantic synonyms into our token pool
    for st in semantic_tokens {
        if !query_tokens.contains(st) {
            query_tokens.push(st.clone());
        }
    }
    
    let mut rows = stmt.query([])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        
        let name_lower = name.to_lowercase();
        let mut score = 0;

        // 1. Exact Full Query Match (highest priority)
        if name_lower == query_lower {
            score += 1000;
        } else if name_lower.contains(&query_lower) {
            score += 500;
        }

        // 2. Multi-word conceptual matching & fuzzy match
        for token in &query_tokens {
            if &name_lower == token {
                score += 100;
            } else if name_lower.contains(token) {
                score += 50;
            } else {
                // Fuzzy match (Levenshtein)
                let dist = levenshtein(&name_lower, token);
                if dist <= 2 && token.len() > 3 {
                    score += 25 - (dist as i32 * 5);
                }
            }
        }
        
        // 3. File path relevance (if query mentions 'database', give bonus to 'database.rs')
        let path_lower = path.to_lowercase();
        for token in &query_tokens {
            if path_lower.contains(token) {
                score += 10;
            }
        }

        if score > 0 {
            results.push(SearchResult { path, name, kind, score });
        }
    }
    
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(50);
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
        let end_line: i64 = row.get(3).unwrap_or(start_line); // Default to start_line if missing
        
        let mut preview = String::new();
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = content.lines().collect();
            let start = (start_line.saturating_sub(context_lines as i64 + 1)).max(0) as usize;
            let end = (end_line + context_lines as i64).min(lines.len() as i64) as usize;
            if start < lines.len() {
                preview = lines[start..end].join("\n");
            }
        }
        
        results.push((path, kind, start_line, preview));
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