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
    pub decorators: Vec<String>,

    pub reverse_dependencies: Vec<String>, // files that rely on symbols
    pub siblings: Vec<String>, // symbols that are defubed ub tge exact same file
    pub forward_dependencies: Vec<String>, // what this file imports
    pub prop_interfaces: Vec<crate::retrieval::SymbolSourceResult>, // bundled prop interfaces
    pub wrapped_by: Vec<String>, // layout files that wrap this route
    pub renders_components: Vec<String>, // components rendered by this file
    pub consumes_hooks: Vec<String>, // hooks consumed by this file
}

impl ContextObject {
    /// Assembles a rich, multi-dimensional context package for a specific symbol
    pub fn assemble(db: &Database, symbol_name: &str) -> Result<Option<Self>> {
        
        // 1. Fetch the primary target's core definition (Distance-0 Context)
        let mut stmt = db.conn.prepare(
            "SELECT symbols.name, symbols.kind, files.path, symbols.start_line, symbols.file_id, symbols.signature
             FROM symbols
             JOIN files ON symbols.file_id = files.id
             WHERE symbols.name = ?1 LIMIT 1"
        )?;

        let target_info = stmt.query_row(rusqlite::params![symbol_name], |row| {
            Ok((
                row.get::<_, String>(0)?, // name
                row.get::<_, String>(1)?, // kind
                row.get::<_, String>(2)?, // path
                row.get::<_, i64>(3)?,    // line_number
                row.get::<_, i64>(4)?,    // file_id
                row.get::<_, Option<String>>(5)?, // signature
            ))
        });

        if symbol_name.trim().is_empty() {
            return Ok(None);
        }

        let (name, kind, path, line_number, file_id, signature) = match target_info {
            Ok(info) => info,
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                // Fallback: check if the 'symbol_name' is actually a file path
                let mut file_stmt = db.conn.prepare("SELECT id, path FROM files WHERE path LIKE ?1 LIMIT 1")?;
                let file_search = format!("%{}%", symbol_name);
                if let Ok((f_id, f_path)) = file_stmt.query_row(rusqlite::params![file_search], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))) {
                    (
                        std::path::Path::new(&f_path).file_name().and_then(|n| n.to_str()).unwrap_or(&f_path).to_string(),
                        "file".to_string(),
                        f_path,
                        1,
                        f_id,
                        None
                    )
                } else {
                    return Ok(None);
                }
            },
            Err(e) => return Err(e),
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

        // 4. Fetch Forward Dependencies (What does this file import?)
        let mut fwd_stmt = db.conn.prepare(
            "SELECT symbols.name 
             FROM edges 
             JOIN symbols ON edges.target_symbol_id = symbols.id
             WHERE edges.source_file_id = ?1 AND edges.kind = 'imports'"
        )?;
        
        let mut fwd_rows = fwd_stmt.query(rusqlite::params![file_id])?;
        let mut forward_dependencies = Vec::new();
        while let Some(row) = fwd_rows.next()? {
            forward_dependencies.push(row.get(0)?);
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
            wrapped_by.push(row.get::<_, String>(0)?);
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

        // 5. Package it all up into our pristine, JSON-ready Context Object
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

        // Make paths absolute
        let current_dir = std::env::current_dir().unwrap_or_default().display().to_string();
        let abs_path = format!("{}/{}", current_dir, path);
        
        let mut abs_rev_deps = Vec::new();
        for p in rev_deps {
            abs_rev_deps.push(format!("{}/{}", current_dir, p));
        }
        
        let mut abs_wrapped_by = Vec::new();
        for p in wrapped_by {
            abs_wrapped_by.push(format!("{}/{}", current_dir, p));
        }

        Ok(Some(ContextObject {
            target_name: name,
            target_kind: kind,
            defining_file: abs_path,
            line_number: line_number as usize,
            signature: final_signature,
            decorators,
            reverse_dependencies: abs_rev_deps,
            siblings,
            forward_dependencies,
            prop_interfaces,
            wrapped_by: abs_wrapped_by,
            renders_components,
            consumes_hooks,
        }))
    }
}