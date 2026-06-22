use serde::{Serialize, Deserialize};
use storage::Database;
use rusqlite::Result;

#[derive(Debug, Serialize, Deserialize)]

pub struct ContextObject {
    pub target_name: String,
    pub target_kind: String,
    pub defining_file: String,
    pub line_number: usize,
    pub signature: Option<String>,
    pub directive: Option<String>,
    pub decorators: Vec<String>,

    pub reverse_dependencies: Vec<String>, // files that rely on symbols
    pub siblings: Vec<String>, // symbols that are defubed ub tge exact same file
    pub forward_dependencies: Vec<String>, // local symbols this file imports
    pub external_imports: Vec<String>, // unresolved or third-party package imports
    pub prop_interfaces: Vec<crate::retrieval::SymbolSourceResult>, // bundled prop interfaces
    pub wrapped_by: Vec<String>, // layout files that wrap this route
    pub renders_components: Vec<String>, // components rendered by this file
    pub consumes_hooks: Vec<String>, // hooks consumed by this file
    pub callers: Vec<String>, // files that call this symbol
    pub callees: Vec<String>, // symbols called by this file

    /// False when the repository's dependency graph has zero edges. When this
    /// is false, all the dependency fields above are NOT a reliable signal of
    /// "no relationships exist" — they mean "the graph was never built", and
    /// any analysis built on top of this context (e.g. impact_analysis) is a
    /// source-only guess, not a real graph traversal.
    pub graph_indexed: bool,

    /// True when `defining_file` has changed on disk since it was last
    /// indexed (its content no longer matches the hash recorded at index
    /// time). `line_number` and any byte-offset-derived data for this symbol
    /// may be wrong when this is true — re-run reindex_workspace (or
    /// `codebroker reindex-incremental` on this file) before trusting them.
    pub stale: bool,
}

impl ContextObject {
    /// Assembles a rich, multi-dimensional context package for a specific symbol
    pub fn assemble(db: &Database, symbol_name: &str) -> Result<Option<Self>> {
        Self::assemble_scoped(db, symbol_name, None)
    }

    /// Like `assemble`, but when `file_hint` is given, only resolves a symbol
    /// defined in a file whose path contains that substring. Callers should
    /// use `query::engine::find_symbol_candidates` first to check whether a
    /// name is ambiguous before relying on this to silently disambiguate —
    /// this only narrows the SQL query, it doesn't itself report ambiguity.
    pub fn assemble_scoped(db: &Database, symbol_name: &str, file_hint: Option<&str>) -> Result<Option<Self>> {

        if symbol_name.trim().is_empty() {
            return Ok(None);
        }

        // 1. Fetch the primary target's core definition (Distance-0 Context)
        let target_info = if let Some(hint) = file_hint {
            let mut stmt = db.conn.prepare(
                "SELECT symbols.name, symbols.kind, files.path, symbols.start_line, symbols.file_id, symbols.signature, files.directive, files.content_hash
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 AND files.path LIKE ?2 LIMIT 1"
            )?;
            let pattern = format!("%{}%", hint);
            stmt.query_row(rusqlite::params![symbol_name, pattern], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<String>>(7)?,
                ))
            })
        } else {
            let mut stmt = db.conn.prepare(
                "SELECT symbols.name, symbols.kind, files.path, symbols.start_line, symbols.file_id, symbols.signature, files.directive, files.content_hash
                 FROM symbols
                 JOIN files ON symbols.file_id = files.id
                 WHERE symbols.name = ?1 LIMIT 1"
            )?;
            stmt.query_row(rusqlite::params![symbol_name], |row| {
                Ok((
                    row.get::<_, String>(0)?, // name
                    row.get::<_, String>(1)?, // kind
                    row.get::<_, String>(2)?, // path
                    row.get::<_, i64>(3)?,    // line_number
                    row.get::<_, i64>(4)?,    // file_id
                    row.get::<_, Option<String>>(5)?, // signature
                    row.get::<_, Option<String>>(6)?, // directive
                    row.get::<_, Option<String>>(7)?, // content_hash
                ))
            })
        };

        let (name, kind, path, line_number, file_id, signature, directive, indexed_content_hash) = match target_info {
            Ok(info) => info,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Fallback: check if the 'symbol_name' is actually a file path
                let mut file_stmt = db.conn.prepare("SELECT id, path, directive, content_hash FROM files WHERE path LIKE ?1 LIMIT 1")?;
                let file_search = format!("%{}%", symbol_name);
                if let Ok((f_id, f_path, f_dir, f_hash)) = file_stmt.query_row(rusqlite::params![file_search], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, Option<String>>(2)?, row.get::<_, Option<String>>(3)?))) {
                    (symbol_name.to_string(), "file".to_string(), db.resolve_path(&f_path), 1, f_id, None, f_dir, f_hash)
                } else {
                    return Ok(None);
                }
            },
            Err(e) => return Err(e),
        };

        // A file edited since indexing shifts every byte offset (and often
        // line numbers) for symbols defined in it — surface that instead of
        // letting a caller silently trust a `line_number` that no longer
        // points at this symbol.
        let stale = match &indexed_content_hash {
            Some(stored_hash) => std::fs::read(db.resolve_path(&path))
                .map(|content| storage::hash_content(&content) != *stored_hash)
                .unwrap_or(false),
            None => false,
        };

        // 2. Fetch the Blast Radius / Reverse Dependencies (Distance-1 Context)
        // We can completely reuse the awesome engine we built in Phase 1!
        let rev_deps = crate::engine::find_dependents(db, symbol_name)
            .unwrap_or_else(|_| Vec::new());

        // 3. Fetch the Siblings (Immediate Neighborhood Context)
        // We want to know what else lives in the exact same file, excluding itself and raw imports.
        let mut sib_stmt = db.conn.prepare(
            "SELECT name FROM symbols 
             WHERE file_id = ?1 AND name != ?2 AND kind != 'import'"
        )?;
        
        let mut sib_rows = sib_stmt.query(rusqlite::params![file_id, symbol_name])?;
        let mut siblings = Vec::new();
        while let Some(row) = sib_rows.next()? {
            siblings.push(row.get(0)?);
        }

        // 4. Fetch Forward Dependencies (Local symbols this file imports)
        let mut fwd_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'imports'"
        )?;
        
        let mut fwd_rows = fwd_stmt.query(rusqlite::params![file_id])?;
        let mut forward_dependencies = Vec::new();
        while let Some(row) = fwd_rows.next()? {
            forward_dependencies.push(row.get::<_, String>(0)?);
        }

        // 4.1 Fetch External / Unresolved Imports
        // Get all raw imports and diff them against the resolved local symbols
        let mut ext_stmt = db.conn.prepare(
            "SELECT name, source FROM raw_imports WHERE file_id = ?1 AND (kind = 'imports' OR kind = 'renders_component' OR kind = 'consumes_hook')"
        )?;
        let mut ext_rows = ext_stmt.query(rusqlite::params![file_id])?;
        let mut external_imports = Vec::new();
        while let Some(row) = ext_rows.next()? {
            let name: String = row.get(0)?;
            let source: Option<String> = row.get(1)?;
            
            // If the raw import name is NOT in our resolved local edges, it's external
            if !forward_dependencies.contains(&name) {
                if let Some(src) = source {
                    // Only add the 'from' if it's explicitly available, preventing redundancy
                    if !src.is_empty() {
                        external_imports.push(format!("{} (from {})", name, src));
                    } else {
                        external_imports.push(name);
                    }
                } else {
                    external_imports.push(name);
                }
            }
        }

        // 4.5 Fetch Props Interfaces
        // Find 'accepts_props' edges from this file
        let mut props_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'accepts_props'"
        )?;
        let mut props_rows = props_stmt.query(rusqlite::params![file_id])?;
        let mut prop_interfaces = Vec::new();
        while let Some(row) = props_rows.next()? {
            let prop_name: String = row.get(0)?;
            if let Ok(srcs) = crate::retrieval::read_symbol_source(db, &prop_name, false) {
                prop_interfaces.extend(srcs);
            }
        }

        // Schema Auto-Expansion (Python & TS Types)
        let deps = crate::retrieval::fetch_data_model_dependencies(db, &name, file_id, signature.as_deref());
        prop_interfaces.extend(deps);

        // 4.6 Fetch Wrappers
        // Find 'wraps_route' edges pointing to THIS symbol
        let mut wrap_stmt = db.conn.prepare(
            "SELECT files.path 
             FROM edges 
             JOIN files ON edges.source_file_id = files.id
             WHERE edges.target_symbol_id = (SELECT id FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1) 
             AND edges.kind = 'wraps_route'"
        )?;
        let mut wrap_rows = wrap_stmt.query(rusqlite::params![file_id, symbol_name])?;
        let mut wrapped_by = Vec::new();
        while let Some(row) = wrap_rows.next()? {
            let path: String = row.get(0)?;
            wrapped_by.push(db.resolve_path(&path));
        }

        // 4.7 Fetch Rendered Components
        let mut render_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'renders_component'"
        )?;
        let mut render_rows = render_stmt.query(rusqlite::params![file_id])?;
        let mut renders_components = Vec::new();
        while let Some(row) = render_rows.next()? {
            renders_components.push(row.get::<_, String>(0)?);
        }

        // 4.8 Fetch Consumed Hooks
        let mut hook_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'consumes_hook'"
        )?;
        let mut hook_rows = hook_stmt.query(rusqlite::params![file_id])?;
        let mut consumes_hooks = Vec::new();
        while let Some(row) = hook_rows.next()? {
            consumes_hooks.push(row.get::<_, String>(0)?);
        }
        
        let mut raw_hook_stmt = db.conn.prepare(
            "SELECT name FROM raw_imports WHERE file_id = ?1 AND kind = 'consumes_hook'"
        )?;
        let mut raw_hook_rows = raw_hook_stmt.query(rusqlite::params![file_id])?;
        while let Some(row) = raw_hook_rows.next()? {
            let hook_name: String = row.get(0)?;
            if !consumes_hooks.contains(&hook_name) {
                consumes_hooks.push(hook_name);
            }
        }

        // Decorator Extraction
        let mut final_signature = signature.clone();
        let mut decorators = Vec::new();
        if let Some(sig_str) = &signature {
            let parts: Vec<&str> = sig_str.split_whitespace().collect();
            let mut cleaned_parts = Vec::new();
            for part in parts {
                if part.starts_with('@') {
                    decorators.push(part.to_string());
                } else {
                    cleaned_parts.push(part);
                }
            }
            if cleaned_parts.is_empty() {
                final_signature = None;
            } else {
                final_signature = Some(cleaned_parts.join(" "));
            }
        }

        // 4.9 Fetch Callers
        let mut callers_stmt = db.conn.prepare(
            "SELECT files.path 
             FROM edges 
             JOIN files ON edges.source_file_id = files.id
             WHERE edges.target_symbol_id = (SELECT id FROM symbols WHERE file_id = ?1 AND name = ?2 LIMIT 1) 
             AND edges.kind = 'calls'"
        )?;
        let mut callers_rows = callers_stmt.query(rusqlite::params![file_id, name])?;
        let mut callers = Vec::new();
        while let Some(row) = callers_rows.next()? {
            let path: String = row.get(0)?;
            callers.push(db.resolve_path(&path));
        }

        // 4.10 Fetch Callees
        let mut callees_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'calls'"
        )?;
        let mut callees_rows = callees_stmt.query(rusqlite::params![file_id])?;
        let mut callees = Vec::new();
        while let Some(row) = callees_rows.next()? {
            callees.push(row.get::<_, String>(0)?);
        }

        let total_edges: i64 = db.conn.query_row("SELECT COUNT(*) FROM edges", [], |r| r.get(0)).unwrap_or(0);

        Ok(Some(ContextObject {
            target_name: name,
            target_kind: kind,
            defining_file: db.resolve_path(&path),
            line_number: line_number as usize,
            signature: final_signature,
            directive,
            decorators,
            reverse_dependencies: rev_deps,
            siblings,
            forward_dependencies,
            external_imports,
            prop_interfaces,
            wrapped_by,
            renders_components,
            consumes_hooks,
            callers,
            callees,
            graph_indexed: total_edges > 0,
            stale,
        }))
    }
}