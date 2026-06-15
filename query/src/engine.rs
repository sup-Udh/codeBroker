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