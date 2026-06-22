use storage::Database;
use serde::{Serialize, Deserialize};
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::fs;

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

/// Collapses all whitespace runs to single spaces. This catches near-verbatim
/// copy-pasted logic across files even when indentation/line breaks differ,
/// without requiring an actual call/import edge between the two copies (the
/// dependency graph only links things that reference each other; copy-pasted
/// logic with no shared call site is invisible to it). It will NOT catch
/// duplicates that were copy-pasted with renamed variables — that needs
/// AST-level comparison, which is out of scope here.
fn normalize(source: &str) -> String {
    source.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Scans all indexed symbols for near-duplicate logic: function/component
/// bodies whose normalized source text is byte-identical but which live in
/// different files. `min_normalized_len` filters out trivial bodies (e.g.
/// one-line getters) that would otherwise produce noisy false positives.
pub fn find_duplicate_logic(db: &Database, min_normalized_len: usize, path_scope: Option<&str>) -> Result<DuplicateLogicReport, String> {
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
            if !rel_path.contains(scope) {
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
        let normalized = normalize(&source);
        if normalized.len() < min_normalized_len {
            continue;
        }

        symbols_scanned += 1;

        let mut hasher = DefaultHasher::new();
        normalized.hash(&mut hasher);
        let hash = hasher.finish();

        let entry = groups.entry(hash).or_insert_with(|| (normalized.len(), Vec::new(), HashSet::new()));
        entry.2.insert(abs_path.clone());
        entry.1.push(DuplicateMember {
            symbol_name: name,
            kind,
            file_path: abs_path,
            start_line: start_line as usize,
            end_line: end_line as usize,
        });
    }

    let mut result_groups: Vec<DuplicateGroup> = groups
        .into_iter()
        .filter(|(_, (_, members, files))| members.len() > 1 && files.len() > 1)
        .map(|(_, (normalized_length, members, _))| DuplicateGroup { normalized_length, members })
        .collect();

    result_groups.sort_by(|a, b| b.normalized_length.cmp(&a.normalized_length));

    Ok(DuplicateLogicReport {
        symbols_scanned,
        duplicate_groups_found: result_groups.len(),
        groups: result_groups,
    })
}
