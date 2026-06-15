use serde::{Serialize, Deserialize};
use storage::Database;
use rusqlite::Result;

#[derive(Debug, Serialize, Deserialize)]

pub struct ContextObject {
    pub target_name: String,
    pub target_kind: String,
    pub defining_file: String,
    pub line_number: usize,

    pub reverse_dependencies: Vec<String>, // files that rely on symbols

    pub siblings: Vec<String>, // symbols that are defubed ub tge exact same file

    pub forward_dependencies: Vec<String>, // what this file imports


}

impl ContextObject {
    /// Assembles a rich, multi-dimensional context package for a specific symbol
    pub fn assemble(db: &Database, symbol_name: &str) -> Result<Option<Self>> {
        
        // 1. Fetch the primary target's core definition (Distance-0 Context)
        let mut stmt = db.conn.prepare(
            "SELECT symbols.name, symbols.kind, files.path, symbols.line_number, symbols.file_id
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
            ))
        });

        let (name, kind, path, line_number, file_id) = match target_info {
            Ok(info) => info,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
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

        // 5. Package it all up into our pristine, JSON-ready Context Object
        Ok(Some(ContextObject {
            target_name: name,
            target_kind: kind,
            defining_file: path,
            line_number: line_number as usize,
            reverse_dependencies: rev_deps,
            forward_dependencies,
            siblings,
        }))
    }
}