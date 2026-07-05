use serde::{Deserialize, Serialize};
use std::fs;
use storage::Database;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SymbolSourceResult {
    pub symbol_name: String,
    pub symbol_kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub source: String,
    pub directive: Option<String>,
    pub attributes: Vec<String>,
    pub metadata: Option<String>,
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

pub fn read_symbol_source(
    db: &Database,
    symbol: &str,
    include_dependencies: bool,
) -> Result<Vec<SymbolSourceResult>, String> {
    read_symbol_source_scoped(db, symbol, include_dependencies, None)
}

/// Like `read_symbol_source`, but when `file_hint` is given, only matches a
/// symbol defined in a file whose path contains that substring — used to
/// disambiguate a name like "GET" that's defined in many files once the
/// caller has picked one from `find_symbol_candidates`.
pub fn read_symbol_source_scoped(
    db: &Database,
    symbol: &str,
    include_dependencies: bool,
    file_hint: Option<&str>,
) -> Result<Vec<SymbolSourceResult>, String> {
    let (mut stmt, params_vec): (rusqlite::Statement, Vec<Box<dyn rusqlite::ToSql>>) = if let Some(
        hint,
    ) =
        file_hint
    {
        let stmt = db.conn.prepare(
            "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, symbols.signature, symbols.attributes, symbols.metadata, files.content_hash
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.name = ?1 AND INSTR(files.path, ?2) > 0 LIMIT 5"
        ).map_err(|e| e.to_string())?;
        (
            stmt,
            vec![Box::new(symbol.to_string()), Box::new(hint.to_string())],
        )
    } else {
        let stmt = db.conn.prepare(
            "SELECT files.path, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, symbols.signature, symbols.attributes, symbols.metadata, files.content_hash
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.name = ?1 LIMIT 5"
        ).map_err(|e| e.to_string())?;
        (stmt, vec![Box::new(symbol.to_string())])
    };

    let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec.iter().map(|b| b.as_ref()).collect();
    let mut rows = stmt
        .query(param_refs.as_slice())
        .map_err(|e| e.to_string())?;
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
        let attributes_str: Option<String> = row.get(7).unwrap_or(None);
        let attributes = attributes_str
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default();
        let metadata: Option<String> = row.get(8).unwrap_or(None);
        let indexed_content_hash: Option<String> = row.get(9).unwrap_or(None);

        let mut source = String::new();
        let mut stale = false;
        if let Ok(content) = fs::read(&abs_path) {
            // The file may have been edited since `start_byte`/`end_byte` were
            // computed. Those offsets are only meaningful against the exact
            // content they were derived from — applying them to changed
            // content silently yields a misaligned (and often mid-token)
            // substring instead of an error, so check first.
            if let Some(indexed_hash) = &indexed_content_hash {
                if *indexed_hash != storage::hash_content(&content) {
                    stale = true;
                }
            }

            if !stale {
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
        }

        let mut source_unavailable = None;
        let mut reason = None;
        if stale {
            source_unavailable = Some(true);
            reason = Some("Index is stale: this file changed on disk since it was last indexed, so the stored byte offsets no longer line up with its content. Re-run reindex_workspace (or `codebroker reindex-incremental` on this file) before trusting symbol boundaries.".to_string());
            source = "<ERROR: Stale index. This file was modified after indexing; the stored start/end byte offsets no longer match the file on disk and would return a corrupted snippet. Reindex the workspace, then retry.>".to_string();
        } else if source.is_empty() {
            source_unavailable = Some(true);
            reason = Some(
                "Failed to extract code snippet using byte bounds. Please use native file Read."
                    .to_string(),
            );
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
            attributes,
            metadata,
            is_dependency: false,
            source_unavailable,
            reason,
        });

        if include_dependencies {
            let mut file_id_stmt = db
                .conn
                .prepare("SELECT id FROM files WHERE path = ?1 LIMIT 1")
                .unwrap();
            if let Ok(file_id) = file_id_stmt
                .query_row(rusqlite::params![file_path], |r: &rusqlite::Row| {
                    Ok(r.get::<_, i64>(0).unwrap_or(0))
                })
            {
                // To fetch dependencies, we need the signature. Since read_symbol_source doesn't fetch signature by default,
                // we'll quickly query the signature from the symbols table for this specific symbol id.
                let mut sig_stmt = db
                    .conn
                    .prepare(
                        "SELECT signature FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1",
                    )
                    .unwrap();
                let signature: Option<String> = sig_stmt
                    .query_row(rusqlite::params![file_id, symbol], |r: &rusqlite::Row| {
                        Ok(r.get::<_, Option<String>>(0).unwrap_or(None))
                    })
                    .unwrap_or(None);

                let deps = fetch_data_model_dependencies(db, symbol, file_id, signature.as_deref());
                results.extend(deps);
            }
        }
    }

    Ok(results)
}

/// Retrieves the source for a symbol using its primary key `symbol_id`.
/// Bypasses ambiguous name resolution and path matching logic.
pub fn read_symbol_source_by_id(
    db: &Database,
    symbol_id: i64,
    include_dependencies: bool,
) -> Result<Vec<SymbolSourceResult>, String> {
    let mut stmt = db.conn.prepare(
        "SELECT files.path, symbols.name, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, symbols.signature, symbols.attributes, symbols.metadata, files.content_hash
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.id = ?1 LIMIT 1"
    ).map_err(|e| e.to_string())?;

    let mut rows = stmt
        .query([&symbol_id])
        .map_err(|e| e.to_string())?;
    let mut results = Vec::new();

    while let Some(row) = rows.next().unwrap_or(None) {
        let file_path: String = row.get(0).unwrap_or_default();
        let symbol_name: String = row.get(1).unwrap_or_default();
        let abs_path = db.resolve_path(&file_path);
        let kind: String = row.get(2).unwrap_or_default();
        let start_line: i64 = row.get(3).unwrap_or(0);
        let end_line: i64 = row.get(4).unwrap_or(start_line);
        let start_byte: i64 = row.get(5).unwrap_or(0);
        let end_byte: i64 = row.get(6).unwrap_or(start_byte);
        let directive: Option<String> = row.get(7).unwrap_or(None);
        let attributes_str: Option<String> = row.get(8).unwrap_or(None);
        let attributes = attributes_str
            .map(|s| serde_json::from_str(&s).unwrap_or_default())
            .unwrap_or_default();
        let metadata: Option<String> = row.get(9).unwrap_or(None);
        let indexed_content_hash: Option<String> = row.get(10).unwrap_or(None);

        let mut source = String::new();
        let mut stale = false;
        if let Ok(content) = fs::read(&abs_path) {
            if let Some(indexed_hash) = &indexed_content_hash {
                if *indexed_hash != storage::hash_content(&content) {
                    stale = true;
                }
            }

            if !stale {
                let start = start_byte as usize;
                let end = end_byte as usize;
                if start < end && end <= content.len() {
                    source = String::from_utf8_lossy(&content[start..end]).to_string();
                } else {
                    let text = String::from_utf8_lossy(&content);
                    let lines: Vec<&str> = text.lines().collect();
                    let s_idx = (start_line.saturating_sub(1)).max(0) as usize;
                    let e_idx = end_line.min(lines.len() as i64) as usize;
                    if s_idx < lines.len() {
                        source = lines[s_idx..e_idx].join("\n");
                    }
                }
            }
        }

        let mut source_unavailable = None;
        let mut reason = None;
        if stale {
            source_unavailable = Some(true);
            reason = Some("Index is stale: this file changed on disk since it was last indexed, so the stored byte offsets no longer line up with its content. Re-run reindex_workspace (or `codebroker reindex-incremental` on this file) before trusting symbol boundaries.".to_string());
            source = "<ERROR: Stale index. This file was modified after indexing; the stored start/end byte offsets no longer match the file on disk and would return a corrupted snippet. Reindex the workspace, then retry.>".to_string();
        } else if source.is_empty() {
            source_unavailable = Some(true);
            reason = Some("Failed to extract code snippet using byte bounds. Please use native file Read.".to_string());
            source = "<ERROR: Source extraction failed. The file was read successfully but the requested byte bounds were invalid. Please fall back to the native Read tool using the file path provided.>".to_string();
        }

        results.push(SymbolSourceResult {
            symbol_name: symbol_name.clone(),
            symbol_kind: kind,
            file_path: abs_path,
            start_line: start_line as usize,
            end_line: end_line as usize,
            source,
            directive,
            attributes,
            metadata,
            is_dependency: false,
            source_unavailable,
            reason,
        });

        if include_dependencies {
            let mut file_id_stmt = db
                .conn
                .prepare("SELECT id FROM files WHERE path = ?1 LIMIT 1")
                .unwrap();
            if let Ok(file_id) = file_id_stmt
                .query_row(rusqlite::params![file_path], |r: &rusqlite::Row| {
                    Ok(r.get::<_, i64>(0).unwrap_or(0))
                })
            {
                let mut dep_stmt = db.conn.prepare(
                    "SELECT target_symbol_id, kind FROM edges WHERE source_file_id = ?1 AND (source_symbol_id = ?2 OR source_symbol_id IS NULL) LIMIT 15"
                ).unwrap();

                if let Ok(mut dep_rows) =
                    dep_stmt.query(rusqlite::params![file_id, symbol_id])
                {
                    while let Ok(Some(dep_row)) = dep_rows.next() {
                        let target_symbol_id: i64 = dep_row.get(0).unwrap_or(0);
                        let edge_kind: String = dep_row.get(1).unwrap_or_default();

                        let mut sym_stmt = db.conn.prepare(
                            "SELECT files.path, symbols.name, symbols.kind, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte, symbols.signature, symbols.attributes, symbols.metadata, files.content_hash
                             FROM symbols
                             JOIN files ON symbols.file_id = files.id
                             WHERE symbols.id = ?1"
                        ).unwrap();

                        if let Ok(mut sym_rows) = sym_stmt.query([target_symbol_id]) {
                            if let Ok(Some(sym_row)) = sym_rows.next() {
                                let d_file_path: String = sym_row.get(0).unwrap_or_default();
                                let d_symbol_name: String = sym_row.get(1).unwrap_or_default();
                                let d_abs_path = db.resolve_path(&d_file_path);
                                let d_kind: String = sym_row.get(2).unwrap_or_default();
                                let d_start_line: i64 = sym_row.get(3).unwrap_or(0);
                                let d_end_line: i64 = sym_row.get(4).unwrap_or(d_start_line);
                                let d_start_byte: i64 = sym_row.get(5).unwrap_or(0);
                                let d_end_byte: i64 = sym_row.get(6).unwrap_or(d_start_byte);
                                let d_directive: Option<String> = sym_row.get(7).unwrap_or(None);
                                let d_attributes_str: Option<String> =
                                    sym_row.get(8).unwrap_or(None);
                                let d_attributes = d_attributes_str
                                    .map(|s| serde_json::from_str(&s).unwrap_or_default())
                                    .unwrap_or_default();
                                let d_metadata: Option<String> = sym_row.get(9).unwrap_or(None);
                                let d_indexed_hash: Option<String> = sym_row.get(10).unwrap_or(None);

                                let mut d_source = String::new();
                                let mut d_stale = false;
                                if let Ok(d_content) = fs::read(&d_abs_path) {
                                    if let Some(ih) = &d_indexed_hash {
                                        if *ih != storage::hash_content(&d_content) {
                                            d_stale = true;
                                        }
                                    }
                                    if !d_stale {
                                        let ds = d_start_byte as usize;
                                        let de = d_end_byte as usize;
                                        if ds < de && de <= d_content.len() {
                                            d_source =
                                                String::from_utf8_lossy(&d_content[ds..de])
                                                    .to_string();
                                        } else {
                                            let text = String::from_utf8_lossy(&d_content);
                                            let lines: Vec<&str> = text.lines().collect();
                                            let s_idx = (d_start_line.saturating_sub(1)).max(0) as usize;
                                            let e_idx = d_end_line.min(lines.len() as i64) as usize;
                                            if s_idx < lines.len() {
                                                d_source = lines[s_idx..e_idx].join("\n");
                                            }
                                        }
                                    }
                                }

                                let mut d_unavail = None;
                                let mut d_reason = None;
                                if d_stale {
                                    d_unavail = Some(true);
                                    d_reason = Some("Dependency file is stale on disk".to_string());
                                } else if d_source.is_empty() {
                                    d_unavail = Some(true);
                                    d_reason = Some("Failed to extract dependency snippet".to_string());
                                }

                                results.push(SymbolSourceResult {
                                    symbol_name: d_symbol_name,
                                    symbol_kind: format!("{} (via {})", d_kind, edge_kind),
                                    file_path: d_abs_path,
                                    start_line: d_start_line as usize,
                                    end_line: d_end_line as usize,
                                    source: d_source,
                                    directive: d_directive,
                                    attributes: d_attributes,
                                    metadata: d_metadata,
                                    is_dependency: true,
                                    source_unavailable: d_unavail,
                                    reason: d_reason,
                                });
                            }
                        }
                    }
                }
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

pub fn fetch_data_model_dependencies(
    db: &Database,
    symbol_name: &str,
    file_id: i64,
    signature: Option<&str>,
) -> Vec<SymbolSourceResult> {
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
            if word.is_empty()
                || word.chars().next().unwrap().is_lowercase()
                || word == "Depends"
                || word == "Session"
                || word == symbol_name
            {
                continue;
            }
            // A generic type repeated across params (Map<string, User>, User[], User)
            // would otherwise get expanded once per occurrence.
            if !processed_words.insert(word) {
                continue;
            }
            // Check if this word exists as a symbol
            let mut check_stmt = db
                .conn
                .prepare("SELECT name, file_id FROM symbols WHERE name = ?1 LIMIT 1")
                .unwrap();
            if let Ok((_name, found_file_id)) =
                check_stmt.query_row(rusqlite::params![word], |row| {
                    Ok((
                        row.get::<_, String>(0).unwrap(),
                        row.get::<_, i64>(1).unwrap(),
                    ))
                })
            {
                if let Ok(mut srcs) = read_symbol_source(db, word, false) {
                    // false to prevent infinite recursion
                    for src in &mut srcs {
                        src.is_dependency = true;
                    }
                    deps.extend(srcs);
                }

                if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
                    break;
                }

                // Follow inherits edges!
                let mut inherits_stmt = db.conn.prepare(
                    "SELECT symbols.name FROM edges JOIN symbols ON edges.target_symbol_id = symbols.id WHERE edges.source_file_id = ?1 AND edges.kind = 'inherits'"
                ).unwrap();
                let mut inherits_rows = inherits_stmt
                    .query(rusqlite::params![found_file_id])
                    .unwrap();
                while let Some(i_row) = inherits_rows.next().unwrap_or(None) {
                    if deps.len() >= MAX_DEPENDENCY_EXPANSIONS {
                        break;
                    }
                    let i_name: String = i_row.get(0).unwrap();
                    if let Ok(mut srcs) = read_symbol_source(db, &i_name, false) {
                        for src in &mut srcs {
                            src.is_dependency = true;
                        }
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
                for src in &mut srcs {
                    src.is_dependency = true;
                }
                deps.extend(srcs);
            }
        }
    }

    deps
}

/// Turns "Is a directory (os error 21)" into an actionable message naming
/// the route/index file the caller probably meant. Agents frequently pass a
/// route's directory (e.g. a Next.js "app/api/rooms" segment) instead of the
/// file inside it, since that's how the route is referred to conversationally.
fn directory_hint_error(path: &std::path::Path) -> String {
    let candidates: Vec<String> = fs::read_dir(path)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| e.file_name().to_string_lossy().to_string())
                .collect()
        })
        .unwrap_or_default();
    if candidates.is_empty() {
        format!("'{}' is a directory, not a file.", path.display())
    } else {
        format!(
            "'{}' is a directory, not a file. Files in it: {}. Pass one of these as the file path.",
            path.display(),
            candidates.join(", ")
        )
    }
}

pub fn read_file_snippet(
    path: &str,
    start_line: usize,
    end_line: usize,
) -> Result<FileSnippetResult, String> {
    let path_obj = std::path::Path::new(path);
    if path_obj.is_dir() {
        return Err(directory_hint_error(path_obj));
    }
    let content = fs::read_to_string(path).map_err(|e| format!("Failed to read file: {}", e))?;
    let lines: Vec<&str> = content.lines().collect();
    let total_lines = lines.len();

    // Validate the requested range against the real file length instead of
    // silently returning an empty `source` for an out-of-range request — an
    // empty string was indistinguishable from "this region really is blank",
    // which could mislead a caller into thinking a file region was empty.
    if start_line == 0 {
        return Err("start_line is 1-based; it must be >= 1.".to_string());
    }
    if start_line > total_lines {
        return Err(format!(
            "start_line {} exceeds file length ({} lines) for '{}'.",
            start_line, total_lines, path
        ));
    }
    if end_line < start_line {
        return Err(format!(
            "end_line {} is before start_line {} for '{}'.",
            end_line, start_line, path
        ));
    }

    let s_idx = start_line.saturating_sub(1);
    let e_idx = end_line.min(total_lines);
    let source = lines[s_idx..e_idx].join("\n");

    Ok(FileSnippetResult {
        file_path: path.to_string(),
        // Report the line actually returned, clamped to the file, so a caller
        // that over-asked (e.g. end_line past EOF) sees what it really got.
        start_line,
        end_line: e_idx,
        source,
    })
}

pub fn skeletonize_file(
    db: &Database,
    file_path: &str,
    target_symbol: Option<&str>,
) -> Result<String, String> {
    // Find file_id by resolving paths to absolute and comparing
    let mut files_stmt = db
        .conn
        .prepare("SELECT id, path, content_hash FROM files")
        .map_err(|e| e.to_string())?;
    let mut files_rows = files_stmt.query([]).map_err(|e| e.to_string())?;
    let mut file_id = 0;
    let mut indexed_content_hash: Option<String> = None;
    let mut actual_abs_path = String::new();

    let target_path_str = file_path.trim_start_matches("./");

    while let Some(row) = files_rows.next().unwrap_or(None) {
        let id: i64 = row.get(0).unwrap();
        let path: String = row.get(1).unwrap();
        let abs = db.resolve_path(&path);

        // Flexible matching: exact absolute, exact stored relative, or ends with the given path segment
        if abs == file_path
            || path == file_path
            || path.ends_with(target_path_str)
            || abs.ends_with(target_path_str)
        {
            file_id = id;
            indexed_content_hash = row.get(2).unwrap_or(None);
            actual_abs_path = abs;
            break;
        }
    }

    if file_id == 0 {
        // The caller may have passed a directory (e.g. a Next.js segment like
        // "frontend/app") rather than a file. The directory may be nested below
        // the project root (here `frontend/app` actually lives at
        // `OrcaAI/frontend/app`), so resolving the literal string against the
        // root won't find it. Instead, treat the input as a directory segment
        // and list the INDEXED files that live directly inside it — more useful
        // than a raw filesystem listing because it only names files CodeBroker
        // can actually skeletonize.
        let seg = target_path_str.trim_matches('/');
        if !seg.is_empty() {
            let mut children: Vec<String> = Vec::new();
            let mut all_stmt = db
                .conn
                .prepare("SELECT path FROM files")
                .map_err(|e| e.to_string())?;
            let mut all_rows = all_stmt.query([]).map_err(|e| e.to_string())?;
            while let Some(row) = all_rows.next().map_err(|e| e.to_string())? {
                let p: String = row.get(0).unwrap_or_default();
                let norm = p.trim_start_matches("./");
                if let Some(parent) = std::path::Path::new(norm).parent() {
                    let parent = parent.to_string_lossy();
                    if parent == seg || parent.ends_with(&format!("/{}", seg)) {
                        if let Some(fname) = std::path::Path::new(norm)
                            .file_name()
                            .and_then(|n| n.to_str())
                        {
                            children.push(fname.to_string());
                        }
                    }
                }
            }
            if !children.is_empty() {
                children.sort();
                children.dedup();
                return Err(format!(
                    "'{}' is a directory, not a file. Indexed files in it: {}. Pass one of these as the file path.",
                    file_path,
                    children.join(", ")
                ));
            }
        }
        // Last resort: a real on-disk directory with no indexed children.
        for candidate in [db.resolve_path(target_path_str), file_path.to_string()] {
            let p = std::path::Path::new(&candidate);
            if p.is_dir() {
                return Err(directory_hint_error(p));
            }
        }
        return Err(format!("File '{}' not found in index.", file_path));
    }

    let abs_path_obj = std::path::Path::new(&actual_abs_path);
    if abs_path_obj.is_dir() {
        return Err(directory_hint_error(abs_path_obj));
    }
    let content = fs::read(&actual_abs_path).map_err(|e| format!("Failed to read file: {}", e))?;

    // The skeleton is built by walking stored start_byte/end_byte offsets
    // against `content` read just above. If the file changed since indexing,
    // those offsets no longer line up with this content: slicing on them
    // doesn't error, it just glues unrelated fragments together (the tail of
    // one symbol's old range bleeding into the next symbol's new position).
    if let Some(indexed_hash) = &indexed_content_hash {
        if *indexed_hash != storage::hash_content(&content) {
            return Err(format!(
                "Index is stale for '{}': this file changed on disk since it was last indexed, so stored byte offsets no longer match its content. Re-run reindex_workspace (or `codebroker reindex-incremental` on this file) before requesting a skeleton.",
                file_path
            ));
        }
    }

    let mut target_start = 0;
    let mut target_end = 0;
    if let Some(target) = target_symbol {
        let mut target_stmt = db
            .conn
            .prepare(
                "SELECT start_byte, end_byte FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1",
            )
            .map_err(|e| e.to_string())?;
        if let Ok((ts, te)) = target_stmt.query_row(rusqlite::params![file_id, target], |r| {
            Ok((
                r.get::<_, i64>(0).unwrap_or(0),
                r.get::<_, i64>(1).unwrap_or(0),
            ))
        }) {
            target_start = ts;
            target_end = te;
        } else {
            return Err(format!("Symbol '{}' not found in '{}'", target, file_path));
        }
    }

    let mut stmt = db.conn.prepare("SELECT name, start_byte, end_byte, signature, kind FROM symbols WHERE file_id = ?1 ORDER BY start_byte ASC").map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query(rusqlite::params![file_id])
        .map_err(|e| e.to_string())?;

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
