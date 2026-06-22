use storage::Database;
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
    pub is_dependency: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_unavailable: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct FileSnippetResult {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
}

pub fn read_symbol_source(db: &Database, symbol: &str, include_dependencies: bool) -> Result<Vec<SymbolSourceResult>, String> {
    read_symbol_source_scoped(db, symbol, include_dependencies, None)
}

/// Like `read_symbol_source`, but when `file_hint` is given, only matches a
/// symbol defined in a file whose path contains that substring — used to
/// disambiguate a name like "GET" that's defined in many files once the
/// caller has picked one from `find_symbol_candidates`.
pub fn read_symbol_source_scoped(db: &Database, symbol: &str, include_dependencies: bool, file_hint: Option<&str>) -> Result<Vec<SymbolSourceResult>, String> {
    let (mut stmt, params_vec): (rusqlite::Statement, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(hint) = file_hint {
        let stmt = db.conn.prepare(
            "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, files.directive, files.route_path, files.route_segment
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 5"
        ).map_err(|e| e.to_string())?;
        let pattern = format!("%{}%", hint);
        (stmt, vec![Box::new(symbol.to_string()), Box::new(pattern)])
    } else {
        let stmt = db.conn.prepare(
            "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, files.directive, files.route_path, files.route_segment
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.name = ?1 LIMIT 5"
        ).map_err(|e| e.to_string())?;
        (stmt, vec![Box::new(symbol.to_string())])
    };

    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let mut rows = stmt.query(param_refs.as_slice()).map_err(|e| e.to_string())?;
    let mut results = Vec::new();
    
    while let Some(row) = rows.next().unwrap_or(None) {
        let file_path: String = row.get(0).unwrap_or_default();
        let abs_path = db.resolve_path(&file_path);
        let kind: String = row.get(1).unwrap_or_default();
        let start_line: i64 = row.get(2).unwrap_or(0);
        let end_line: i64 = row.get(3).unwrap_or(start_line);
        let start_byte: i64 = row.get(4).unwrap_or(0);
        let end_byte: i64 = row.get(5).unwrap_or(start_byte);
        let directive: Option<String> = row.get(6).unwrap_or(None);
        let route_path: Option<String> = row.get(7).unwrap_or(None);
        let route_segment: Option<String> = row.get(8).unwrap_or(None);
        
        let mut source = String::new();
        if let Ok(content) = fs::read(&abs_path) {
            let start = start_byte as usize;
            let end = end_byte as usize;
            if start < end && end <= content.len() {
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
        
        let mut source_unavailable = None;
        let mut reason = None;
        if source.is_empty() {
            source_unavailable = Some(true);
            reason = Some("Failed to extract code snippet using byte bounds. Please use native file Read.".to_string());
            source = "<ERROR: Source extraction failed. The file was read successfully but the requested byte bounds were invalid. Please fall back to the native Read tool using the file path provided.>".to_string();
        }

        results.push(SymbolSourceResult {
            symbol_name: symbol.to_string(),
            symbol_kind: kind,
            file_path: abs_path,
            start_line: start_line as usize,
            end_line: end_line as usize,
            source,
            directive,
            route_path,
            route_segment,
            is_dependency: false,
            source_unavailable,
            reason,
        });
        
        if include_dependencies {
            let mut file_id_stmt = db.conn.prepare("SELECT id FROM files WHERE path = ?1 LIMIT 1").unwrap();
            if let Ok(file_id) = file_id_stmt.query_row(rusqlite::params![file_path], |r: &rusqlite::Row| Ok(r.get::<_, i64>(0).unwrap_or(0))) {
                // To fetch dependencies, we need the signature. Since read_symbol_source doesn't fetch signature by default,
                // we'll quickly query the signature from the symbols table for this specific symbol id.
                let mut sig_stmt = db.conn.prepare("SELECT signature FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1").unwrap();
                let signature: Option<String> = sig_stmt.query_row(rusqlite::params![file_id, symbol], |r: &rusqlite::Row| Ok(r.get::<_, Option<String>>(0).unwrap_or(None))).unwrap_or(None);
                
                let deps = fetch_data_model_dependencies(db, symbol, file_id, signature.as_deref());
                results.extend(deps);
            }
        }
    }
    
    Ok(results)
}

/// Hard cap on how many "data model dependency" symbols a single call expands
/// into full inlined source. Previously unbounded: a signature with many
/// capitalized type words (common in TS generics, e.g. `Map<string, User>` /
/// `User[]` repeated across params) would expand every one of them — and
/// each expansion could itself pull up to 5 candidates (read_symbol_source's
/// own LIMIT 5) since this entry point has no file scoping — silently
/// inflating a single context/patch/impact-analysis call into dozens of
/// inlined source bodies and the token cost that comes with it.
const MAX_DEPENDENCY_EXPANSIONS: usize = 8;

pub fn fetch_data_model_dependencies(db: &Database, symbol_name: &str, file_id: i64, signature: Option<&str>) -> Vec<SymbolSourceResult> {
    let mut deps = Vec::new();
    let mut processed_words: std::collections::HashSet<&str> = std::collections::HashSet::new();

    // Schema Auto-Expansion (Python & TS Types)
    if let Some(sig) = signature {
        let words: Vec<&str> = sig.split(|c: char| !c.is_alphabetic()).collect();
        for word in words {
            if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
                break;
            }
            // Skip the symbol's own name: a capitalized function/handler name
            // (e.g. a Next.js "GET" route export) would otherwise get treated
            // as its own "data model dependency" and re-looked-up with no
            // file scoping, silently resolving to a same-named symbol in a
            // completely different file.
            if word.is_empty() || word.chars().next().unwrap().is_lowercase() || word == "Depends" || word == "Session" || word == symbol_name {
                continue;
            }
            // A generic type repeated across params (Map<string, User>, User[], User)
            // would otherwise get expanded once per occurrence.
            if !processed_words.insert(word) {
                continue;
            }
            // Check if this word exists as a symbol
            let mut check_stmt = db.conn.prepare("SELECT name, file_id FROM symbols WHERE name = ?1 LIMIT 1").unwrap();
            if let Ok((_name, found_file_id)) = check_stmt.query_row(rusqlite::params![word], |row| Ok((row.get::<_, String>(0).unwrap(), row.get::<_, i64>(1).unwrap()))) {
                if let Ok(mut srcs) = read_symbol_source(db, word, false) { // false to prevent infinite recursion
                    for src in &mut srcs { src.is_dependency = true; }
                    deps.extend(srcs);
                }

                if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
                    break;
                }

                // Follow inherits edges!
                let mut inherits_stmt = db.conn.prepare(
                    "SELECT symbols.name FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1 AND edges.kind = 'inherits'"
                ).unwrap();
                let mut inherits_rows = inherits_stmt.query(rusqlite::params![found_file_id]).unwrap();
                while let Some(i_row) = inherits_rows.next().unwrap_or(None) {
                    if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
                        break;
                    }
                    let i_name: String = i_row.get(0).unwrap();
                    if let Ok(mut srcs) = read_symbol_source(db, &i_name, false) {
                        for src in &mut srcs { src.is_dependency = true; }
                        deps.extend(srcs);
                    }
                }
            }
        }
    }

    // Fallback: Also check if THIS file defines inherits or accepts_props edges directly
    let mut direct_edges_stmt = db.conn.prepare(
        "SELECT symbols.name FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1 AND (edges.kind = 'inherits' OR edges.kind = 'accepts_props')"
    ).unwrap();
    let mut direct_rows = direct_edges_stmt.query(rusqlite::params![file_id]).unwrap();
    while let Some(row) = direct_rows.next().unwrap_or(None) {
        if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
            break;
        }
        let name: String = row.get(0).unwrap();
        // Prevent dupes if it was already fetched via signature
        if !deps.iter().any(|d| d.symbol_name == name) {
            if let Ok(mut srcs) = read_symbol_source(db, &name, false) {
                for src in &mut srcs { src.is_dependency = true; }
                deps.extend(srcs);
            }
        }
    }

    deps
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

pub fn skeletonize_file(db: &Database, file_path: &str, target_symbol: Option<&str>) -> Result<String, String> {
    let abs_file_path = db.resolve_path(file_path);
    let content = fs::read(&abs_file_path).map_err(|e| format!("Failed to read file: {}", e))?;
    
    // Find file_id by resolving paths to absolute and comparing
    let mut files_stmt = db.conn.prepare("SELECT id, path FROM files").map_err(|e| e.to_string())?;
    let mut files_rows = files_stmt.query([]).map_err(|e| e.to_string())?;
    let mut file_id = 0;
    while let Some(row) = files_rows.next().unwrap_or(None) {
        let id: i64 = row.get(0).unwrap();
        let path: String = row.get(1).unwrap();
        if db.resolve_path(&path) == abs_file_path {
            file_id = id;
            break;
        }
    }
    
    if file_id == 0 {
        return Err(format!("File '{}' not found in index.", file_path));
    }
    
    let mut target_start = 0;
    let mut target_end = 0;
    if let Some(target) = target_symbol {
        let mut target_stmt = db.conn.prepare("SELECT start_byte, end_byte FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1").map_err(|e| e.to_string())?;
        if let Ok((ts, te)) = target_stmt.query_row(rusqlite::params![file_id, target], |r| Ok((r.get::<_, i64>(0).unwrap_or(0), r.get::<_, i64>(1).unwrap_or(0)))) {
            target_start = ts;
            target_end = te;
        } else {
            return Err(format!("Symbol '{}' not found in '{}'", target, file_path));
        }
    }
    
    let mut stmt = db.conn.prepare("SELECT name, start_byte, end_byte, signature, kind FROM symbols WHERE file_id = ?1 ORDER BY start_byte ASC").map_err(|e| e.to_string())?;
    let mut rows = stmt.query(rusqlite::params![file_id]).map_err(|e| e.to_string())?;

    let mut output = String::new();
    let mut current_byte: usize = 0;

    while let Some(row) = rows.next().unwrap_or(None) {
        let name: String = row.get(0).unwrap_or_default();
        let start_byte: usize = row.get::<_, i64>(1).unwrap_or(0) as usize;
        let end_byte: usize = row.get::<_, i64>(2).unwrap_or(0) as usize;
        let signature: Option<String> = row.get(3).unwrap_or(None);
        let kind: String = row.get(4).unwrap_or_default();

        if start_byte < current_byte {
            continue;
        }

        let mut contains_target = false;
        if target_symbol.is_some() {
            if start_byte <= target_start as usize && end_byte >= target_end as usize {
                contains_target = true;
            }
        }

        if contains_target {
            continue;
        }

        if current_byte <= start_byte && start_byte <= content.len() {
            output.push_str(&String::from_utf8_lossy(&content[current_byte..start_byte]));
        }

        // Variables/constants are typically one-liners; collapsing them into
        // "name { ... }" loses the actual value and is nonsensical for kinds
        // that have no block body at all. Show them as-is instead.
        if kind == "variable" {
            if start_byte <= end_byte && end_byte <= content.len() {
                output.push_str(&String::from_utf8_lossy(&content[start_byte..end_byte]));
            }
            current_byte = end_byte;
            continue;
        }

        let sig = signature.unwrap_or(name);
        output.push_str(&sig);

        if file_path.ends_with(".py") {
            if !sig.trim_end().ends_with(':') {
                output.push_str(":\n    ... ");
            } else {
                output.push_str("\n    ... ");
            }
        } else {
            output.push_str(" { ... }");
        }

        current_byte = end_byte;
    }
    
    if current_byte < content.len() {
        output.push_str(&String::from_utf8_lossy(&content[current_byte..]));
    }
    
    Ok(output)
}
