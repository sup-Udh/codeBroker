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

pub fn search_symbols(db: &Database, keyword: &str) -> Result<Vec<SearchResult>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name LIKE ?1 LIMIT 200"
    )?;
    
    let pattern = format!("%{}%", keyword);
    let mut rows = stmt.query(params![pattern])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let name: String = row.get(1)?;
        let kind: String = row.get(2)?;
        
        let name_lower = name.to_lowercase();
        let keyword_lower = keyword.to_lowercase();
        
        let score = if name_lower == keyword_lower {
            100
        } else if name_lower.starts_with(&keyword_lower) {
            50
        } else {
            10
        };
        
        results.push(SearchResult { path, name, kind, score });
    }
    
    results.sort_by(|a, b| b.score.cmp(&a.score));
    results.truncate(50);
    Ok(results)
}

pub fn find_symbol_exact(db: &Database, name: &str) -> Result<Vec<(String, String, i64, String)>, rusqlite::Error> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.kind, symbols.line_number 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1"
    )?;
    
    let mut rows = stmt.query(params![name])?;
    let mut results = Vec::new();
    while let Some(row) = rows.next()? {
        let path: String = row.get(0)?;
        let kind: String = row.get(1)?;
        let line_number: i64 = row.get(2)?;
        
        let mut preview = String::new();
        if let Ok(content) = std::fs::read_to_string(&path) {
            let lines: Vec<&str> = content.lines().collect();
            let start = (line_number.saturating_sub(4)).max(0) as usize;
            let end = (line_number + 3).min(lines.len() as i64) as usize;
            if start < lines.len() {
                preview = lines[start..end].join("\n");
            }
        }
        
        results.push((path, kind, line_number, preview));
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