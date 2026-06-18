use storage::Database;
use rusqlite::params;
use std::fs;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
pub struct SymbolSourceResult {
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
    pub directive: Option<String>,
    pub route_path: Option<String>,
    pub route_segment: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSnippetResult {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
}

pub fn read_symbol_source(db: &Database, symbol: &str) -> Result<Vec<SymbolSourceResult>, String> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, files.directive, files.route_path, files.route_segment 
         FROM symbols 
         JOIN files ON symbols.file_id = files.id 
         WHERE symbols.name = ?1 LIMIT 5"
    ).map_err(|e| e.to_string())?;
    
    let mut rows = stmt.query(params![symbol]).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    
    while let Some(row) = rows.next().unwrap_or(None) {
        let file_path: String = row.get(0).unwrap_or_default();
        let kind: String = row.get(1).unwrap_or_default();
        let start_line: i64 = row.get(2).unwrap_or(0);
        let end_line: i64 = row.get(3).unwrap_or(start_line);
        let start_byte: i64 = row.get(4).unwrap_or(0);
        let end_byte: i64 = row.get(5).unwrap_or(start_byte);
        let directive: Option<String> = row.get(6).unwrap_or(None);
        let route_path: Option<String> = row.get(7).unwrap_or(None);
        let route_segment: Option<String> = row.get(8).unwrap_or(None);
        
        let mut source = String::new();
        if let Ok(content) = fs::read(&file_path) {
            let start = start_byte as usize;
            let end = end_byte as usize;
            if start <= end && end <= content.len() {
                source = String::from_utf8_lossy(&content[start..end]).to_string();
            } else {
                // Fallback to lines if bytes are messed up
                let text = String::from_utf8_lossy(&content);
                let lines: Vec<&str> = text.lines().collect();
                let s_idx = (start_line.saturating_sub(1)).max(0) as usize;
                let e_idx = end_line.min(lines.len() as i64) as usize;
                if s_idx < lines.len() {
                    source = lines[s_idx..e_idx].join("\n");
                }
            }
        }
        
        results.push(SymbolSourceResult {
            symbol_name: symbol.to_string(),
            symbol_kind: kind,
            file_path,
            start_line: start_line as usize,
            end_line: end_line as usize,
            source,
            directive,
            route_path,
            route_segment,
        });
    }
    
    Ok(results)
}

pub fn read_file_snippet(path: &str, start_line: usize, end_line: usize) -> Result<FileSnippetResult, String> {
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    
    let s_idx = start_line.saturating_sub(1);
    let e_idx = end_line.min(lines.len());
    
    let source = if s_idx < lines.len() {
        lines[s_idx..e_idx].join("\n")
    } else {
        String::new()
    };
    
    Ok(FileSnippetResult {
        file_path: path.to_string(),
        start_line,
        end_line,
        source,
    })
}
