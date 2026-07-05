use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::hash::{Hash, Hasher};
use storage::Database;

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateMember {
    pub symbol_name: String,
    pub kind: String,
    pub file_path: String,
    pub start_line: usize,
    pub end_line: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub normalized_length: usize,
    pub members: Vec<DuplicateMember>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DuplicateLogicReport {
    pub symbols_scanned: usize,
    pub duplicate_groups_found: usize,
    pub groups: Vec<DuplicateGroup>,
}

fn normalize(source: &str, file_path: &str) -> Option<(String, usize)> {
    let extension = std::path::Path::new(file_path)
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if let Some((ast_normalized, count)) = parser::normalize::normalize_snippet(source, extension) {
        Some((ast_normalized, count))
    } else {
        // Fallback for unsupported languages or rejected boilerplates
        let fallback = source.split_whitespace().collect::<Vec<_>>().join(" ");
        if fallback.is_empty() {
            None
        } else {
            // Very rough approximation of complexity if no AST available
            Some((fallback.clone(), fallback.split_whitespace().count()))
        }
    }
}
pub fn find_duplicate_logic(
    db: &Database,
    min_normalized_len: usize,
    path_scope: Option<&str>,
) -> Result<DuplicateLogicReport, String> {
    let mut stmt = db.conn.prepare(
        "SELECT symbols.name, symbols.kind, files.path, symbols.start_line, symbols.end_line, symbols.start_byte, symbols.end_byte
         FROM symbols
         JOIN files ON symbols.file_id = files.id
         WHERE symbols.kind NOT IN ('import', 'jsx_element')"
    ).map_err(|e| e.to_string())?;

    let mut rows = stmt.query([]).map_err(|e| e.to_string())?;

    // hash -> (normalized_len, members, distinct file paths seen)
    let mut groups: HashMap<u64, (usize, Vec<DuplicateMember>, HashSet<String>)> = HashMap::new();
    let mut symbols_scanned = 0usize;

    while let Some(row) = rows.next().map_err(|e| e.to_string())? {
        let name: String = row.get(0).unwrap_or_default();
        let kind: String = row.get(1).unwrap_or_default();
        let rel_path: String = row.get(2).unwrap_or_default();
        let start_line: i64 = row.get(3).unwrap_or(0);
        let end_line: i64 = row.get(4).unwrap_or(0);
        let start_byte: i64 = row.get(5).unwrap_or(0);
        let end_byte: i64 = row.get(6).unwrap_or(0);

        if let Some(scope) = path_scope {
            if !crate::path_matches_scope(&rel_path, scope) {
                continue;
            }
        }

        if end_byte <= start_byte {
            continue;
        }

        let abs_path = db.resolve_path(&rel_path);
        let content = match fs::read(&abs_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let (s, e) = (start_byte as usize, end_byte as usize);
        if e > content.len() || s >= e {
            continue;
        }

        let source = String::from_utf8_lossy(&content[s..e]).to_string();

        // `min_normalized_len` is the minimum AST node count (not character
        // length, despite the name/original design) a symbol's body must
        // have to be considered — small enough to avoid trivial one-liners
        // dominating the report, large enough that renamed-identifier
        // duplicates of a real function still clear it. See the
        // `find_duplicate_logic` MCP tool schema for the caller-facing
        // default.
        let (normalized, node_count) = match normalize(&source, &abs_path) {
            Some(res) => res,
            None => continue,
        };

        if node_count < min_normalized_len {
            continue;
        }

        symbols_scanned += 1;

        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        let hash = hasher.finish();

        let entry = groups
            .entry(hash)
            .or_insert_with(|| (node_count, Vec::new(), HashSet::new()));
        entry.2.insert(abs_path.clone());
        entry.1.push(DuplicateMember {
            symbol_name: name,
            kind,
            file_path: abs_path,
            start_line: start_line as usize,
            end_line: end_line as usize,
        });
    }

    // Two or more members with identical normalized structure is a
    // duplicate group regardless of whether they live in the same file or
    // different ones — copy-pasting a function twice within one file is as
    // much "duplicate logic" as doing it across files, and the tool's own
    // stated purpose (finding copy-pasted logic) doesn't distinguish the two.
    let mut result_groups: Vec<DuplicateGroup> = groups
        .into_iter()
        .filter(|(_, (_, members, _))| members.len() > 1)
        .map(|(_, (normalized_length, members, _))| DuplicateGroup {
            normalized_length,
            members,
        })
        .collect();

    result_groups.sort_by(|a, b| b.normalized_length.cmp(&a.normalized_length));

    Ok(DuplicateLogicReport {
        symbols_scanned,
        duplicate_groups_found: result_groups.len(),
        groups: result_groups,
    })
}
